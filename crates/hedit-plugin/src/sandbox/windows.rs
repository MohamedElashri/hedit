//! Windows process boundary for the shared Lua engine. No Lua policy lives here.
use std::{
    ffi::OsStr,
    fs::File,
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{
        Console::*, JobObjects::*, LibraryLoader::*, Pipes::*, SystemInformation::*,
        SystemServices::SECURITY_DESCRIPTOR_REVISION, Threading::*,
    },
};

const PROCESS_BYTES: usize = 256 * 1024 * 1024;
const CPU_TICKS: i64 = 2 * 10_000_000;
const LIMITS: u32 = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
    | JOB_OBJECT_LIMIT_PROCESS_MEMORY
    | JOB_OBJECT_LIMIT_PROCESS_TIME;
// processthreadsapi.h: prohibit dynamic code and disable extension points.
const MITIGATIONS: u64 = (1u64 << 36) | (1u64 << 32);

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub(crate) fn worker_executable() -> io::Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("HEDIT_LUA_TEST_WORKER") {
        return Ok(PathBuf::from(path));
    }
    let executable = std::env::current_exe()?;
    let directory = executable.parent().ok_or_else(denied)?;
    #[cfg(test)]
    let directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory.parent().ok_or_else(denied)?
    } else {
        directory
    };
    Ok(directory.join("hedit-plugin-worker.exe"))
}

fn environment() -> io::Result<Vec<u16>> {
    // AppContainer creation resolves the per-user profile through LOCALAPPDATA;
    // Windows side-by-side loading requires SystemRoot. Allow only the Windows
    // profile path and OS directory, never PATH, credentials, or other values.
    // Keep discovery free of shell imports so the headless worker links only
    // base system DLLs.
    let local = std::env::var_os("LOCALAPPDATA").ok_or_else(denied)?;
    if !Path::new(&local).is_absolute() {
        return Err(denied());
    }
    let mut directory = vec![0u16; 32768];
    let length =
        unsafe { GetSystemWindowsDirectoryW(directory.as_mut_ptr(), directory.len() as u32) }
            as usize;
    if length == 0 || length >= directory.len() {
        return Err(io::Error::last_os_error());
    }
    let mut environment: Vec<u16> = "LOCALAPPDATA=".encode_utf16().collect();
    environment.extend(local.encode_wide());
    environment.push(0);
    environment.extend("SystemRoot=".encode_utf16());
    environment.extend_from_slice(&directory[..length]);
    environment.extend([0, 0]);
    Ok(environment)
}

fn working_directory() -> io::Result<Vec<u16>> {
    // Use a readable system directory. The worker receives its input through
    // pipes and exposes no filesystem API to Lua.
    let mut directory = vec![0u16; 32768];
    let length =
        unsafe { GetSystemDirectoryW(directory.as_mut_ptr(), directory.len() as u32) } as usize;
    if length == 0 || length >= directory.len() {
        return Err(io::Error::last_os_error());
    }
    directory.truncate(length + 1);
    Ok(directory)
}
fn check(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "The Windows Lua worker is not confined as required.",
    )
}
unsafe fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

type CreateProfile = unsafe extern "system" fn(
    *const u16,
    *const u16,
    *const u16,
    *const SID_AND_ATTRIBUTES,
    u32,
    *mut PSID,
) -> i32;
type DeleteProfile = unsafe extern "system" fn(*const u16) -> i32;
struct ProfileApi {
    module: HMODULE,
    create: CreateProfile,
    delete: DeleteProfile,
}
impl ProfileApi {
    fn new() -> io::Result<Self> {
        // Parent-only APIs are resolved from the trusted system directory. The
        // confined child's import table must not pull in USER32 transitively.
        let module = unsafe {
            LoadLibraryExW(
                wide("userenv.dll").as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(io::Error::last_os_error());
        }
        let create =
            unsafe { GetProcAddress(module, c"CreateAppContainerProfile".as_ptr().cast()) };
        let delete =
            unsafe { GetProcAddress(module, c"DeleteAppContainerProfile".as_ptr().cast()) };
        let (Some(create), Some(delete)) = (create, delete) else {
            unsafe {
                FreeLibrary(module);
            }
            return Err(denied());
        };
        // SAFETY: These documented userenv exports have the signatures above;
        // the module is retained until every call and profile cleanup finishes.
        Ok(Self {
            module,
            create: unsafe {
                mem::transmute::<unsafe extern "system" fn() -> isize, CreateProfile>(create)
            },
            delete: unsafe {
                mem::transmute::<unsafe extern "system" fn() -> isize, DeleteProfile>(delete)
            },
        })
    }
}
impl Drop for ProfileApi {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.module);
        }
    }
}

struct Profile {
    api: ProfileApi,
    name: Vec<u16>,
    sid: PSID,
}
impl Profile {
    fn new(role: &str) -> io::Result<Self> {
        let api = ProfileApi::new()?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let name = wide(format!("hedit.{role}.{}", hex::encode(random)));
        let mut sid = ptr::null_mut();
        let status = unsafe {
            (api.create)(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                ptr::null(),
                0,
                &mut sid,
            )
        };
        if status < 0 {
            return Err(io::Error::other(format!(
                "CreateAppContainerProfile: {status:#x}"
            )));
        }
        if sid.is_null() {
            return Err(denied());
        }
        Ok(Self { api, name, sid })
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        unsafe {
            (self.api.delete)(self.name.as_ptr());
            FreeSid(self.sid);
        }
    }
}

fn token(process: HANDLE) -> io::Result<OwnedHandle> {
    let mut handle = ptr::null_mut();
    check(unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) })?;
    unsafe { owned(handle) }
}
fn token_information(
    token: &OwnedHandle,
    class: TOKEN_INFORMATION_CLASS,
) -> io::Result<Vec<usize>> {
    let mut bytes = 0;
    unsafe {
        GetTokenInformation(token.as_raw_handle(), class, ptr::null_mut(), 0, &mut bytes);
    }
    if bytes == 0 || bytes > 64 * 1024 {
        return Err(denied());
    }
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(mem::size_of::<usize>())];
    check(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    })?;
    Ok(buffer)
}
fn require_token(process: HANDLE, expected: Option<PSID>) -> io::Result<()> {
    let token = token(process)?;
    let enabled = token_information(&token, TokenIsAppContainer)?;
    if enabled[0] == 0 {
        return Err(denied());
    }
    let capabilities = token_information(&token, TokenCapabilities)?;
    if unsafe { *(capabilities.as_ptr().cast::<u32>()) } != 0 {
        return Err(denied());
    }
    let information = token_information(&token, TokenAppContainerSid)?;
    let sid = unsafe {
        &*information
            .as_ptr()
            .cast::<TOKEN_APPCONTAINER_INFORMATION>()
    }
    .TokenAppContainer;
    if sid.is_null() || expected.is_some_and(|expected| unsafe { EqualSid(sid, expected) } == 0) {
        return Err(denied());
    }
    for policy in [ProcessDynamicCodePolicy, ProcessChildProcessPolicy] {
        let mut flags = 0u32;
        check(unsafe {
            GetProcessMitigationPolicy(
                process,
                policy,
                (&mut flags as *mut u32).cast(),
                mem::size_of::<u32>(),
            )
        })?;
        if flags & 1 == 0 {
            return Err(denied());
        }
    }
    Ok(())
}

// Protected DACLs grant the editor access, plus optional container read/execute.
struct Acl(*mut ACL);
impl Drop for Acl {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0.cast());
        }
    }
}
fn private_acl(sid: Option<PSID>) -> io::Result<Acl> {
    private_acl_rights(sid.map(|sid| (sid, FILE_GENERIC_READ | FILE_GENERIC_EXECUTE)))
}
fn private_acl_rights(sid: Option<(PSID, u32)>) -> io::Result<Acl> {
    let token = token(unsafe { GetCurrentProcess() })?;
    let user = token_information(&token, TokenUser)?;
    let owner = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() }.User.Sid;
    let entry = |sid: PSID, rights| EXPLICIT_ACCESS_W {
        grfAccessPermissions: rights,
        grfAccessMode: SET_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: sid.cast(),
            ..Default::default()
        },
    };
    let mut entries = vec![entry(owner, FILE_ALL_ACCESS)];
    if let Some((sid, rights)) = sid {
        entries.push(entry(sid, rights));
    }
    let mut acl = ptr::null_mut();
    let status = unsafe {
        SetEntriesInAclW(
            entries.len() as u32,
            entries.as_ptr(),
            ptr::null(),
            &mut acl,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(Acl(acl))
}
fn image_permissions(path: &Path, sid: PSID) -> io::Result<()> {
    let acl = private_acl(Some(sid))?;
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide(path).as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl.0,
            ptr::null(),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

struct Attributes {
    storage: Vec<usize>,
}
impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &mut size);
        }
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0; size.div_ceil(mem::size_of::<usize>())];
        check(unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), count, 0, &mut size)
        })?;
        Ok(Self { storage })
    }
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn set<T>(&mut self, name: u32, value: &T) -> io::Result<()> {
        check(unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                name as usize,
                (value as *const T).cast(),
                mem::size_of::<T>(),
                ptr::null_mut(),
                ptr::null(),
            )
        })
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.pointer());
        }
    }
}

fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    connection(PIPE_ACCESS_INBOUND, 0, FILE_WRITE_DATA | SYNCHRONIZE)
}
fn connection(
    server_access: u32,
    server_flags: u32,
    client_access: u32,
) -> io::Result<(OwnedHandle, OwnedHandle)> {
    // A child receives the server/read end of stdin. Bound the client's token
    // to identification so that end cannot impersonate the editor's authority.
    // https://learn.microsoft.com/en-us/windows/win32/ipc/impersonating-a-named-pipe-client
    let acl = private_acl(None)?;
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    check(unsafe {
        InitializeSecurityDescriptor(
            (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
            SECURITY_DESCRIPTOR_REVISION,
        )
    })?;
    check(unsafe {
        SetSecurityDescriptorDacl(
            (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
            1,
            acl.0,
            0,
        )
    })?;
    let security = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
        bInheritHandle: 1,
    };
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let name = wide(format!(r"\\.\pipe\hedit-worker-{}", hex::encode(random)));
    let read = unsafe {
        owned(CreateNamedPipeW(
            name.as_ptr(),
            server_access | server_flags | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            &security,
        ))
    }?;
    // FILE_APPEND_DATA aliases FILE_CREATE_PIPE_INSTANCE. Request only the
    // individual write right instead of granting GENERIC_WRITE to the child.
    let write = unsafe {
        owned(CreateFileW(
            name.as_ptr(),
            client_access,
            0,
            &security,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        ))
    }?;
    let mut operation = windows_sys::Win32::System::IO::OVERLAPPED::default();
    let event = if server_flags & FILE_FLAG_OVERLAPPED != 0 {
        Some(unsafe { owned(CreateEventW(ptr::null(), 1, 0, ptr::null())) }?)
    } else {
        None
    };
    operation.hEvent = event
        .as_ref()
        .map_or(ptr::null_mut(), |event| event.as_raw_handle());
    let overlapped = if event.is_some() {
        &mut operation
    } else {
        ptr::null_mut()
    };
    if unsafe { ConnectNamedPipe(read.as_raw_handle(), overlapped) } == 0 {
        match unsafe { GetLastError() } {
            ERROR_PIPE_CONNECTED => {}
            ERROR_IO_PENDING if event.is_some() => {
                if unsafe { WaitForSingleObject(operation.hEvent, 1000) } != WAIT_OBJECT_0 {
                    unsafe {
                        windows_sys::Win32::System::IO::CancelIoEx(read.as_raw_handle(), &operation)
                    };
                    let mut bytes = 0;
                    unsafe {
                        windows_sys::Win32::System::IO::GetOverlappedResult(
                            read.as_raw_handle(),
                            &operation,
                            &mut bytes,
                            1,
                        )
                    };
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Tool pipe connection timed out.",
                    ));
                }
                let mut bytes = 0;
                check(unsafe {
                    windows_sys::Win32::System::IO::GetOverlappedResult(
                        read.as_raw_handle(),
                        &operation,
                        &mut bytes,
                        0,
                    )
                })?;
            }
            error => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
    Ok((read, write))
}

fn job(service: bool, native: bool) -> io::Result<OwnedHandle> {
    let handle = unsafe { owned(CreateJobObjectW(ptr::null(), ptr::null())) }?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = LIMITS
        | if native {
            JOB_OBJECT_LIMIT_JOB_MEMORY
        } else {
            0
        };
    if service {
        limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PRIORITY_CLASS;
        limits.BasicLimitInformation.PriorityClass = BELOW_NORMAL_PRIORITY_CLASS;
    }
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.BasicLimitInformation.PerProcessUserTimeLimit = CPU_TICKS;
    limits.ProcessMemoryLimit = PROCESS_BYTES;
    if native {
        limits.JobMemoryLimit = PROCESS_BYTES;
    }
    check(unsafe {
        SetInformationJobObject(
            handle.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            mem::size_of_val(&limits) as u32,
        )
    })?;
    if service {
        let rate = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { CpuRate: 1000 },
        };
        check(unsafe {
            SetInformationJobObject(
                handle.as_raw_handle(),
                JobObjectCpuRateControlInformation,
                (&rate as *const JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                mem::size_of_val(&rate) as u32,
            )
        })?;
    }
    Ok(handle)
}
fn require_job(handle: HANDLE) -> io::Result<()> {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    check(unsafe {
        QueryInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            mem::size_of_val(&limits) as u32,
            ptr::null_mut(),
        )
    })?;
    let basic = limits.BasicLimitInformation;
    if basic.LimitFlags & LIMITS != LIMITS
        || basic.ActiveProcessLimit != 1
        || limits.ProcessMemoryLimit == 0
        || limits.ProcessMemoryLimit > PROCESS_BYTES
        || basic.PerProcessUserTimeLimit <= 0
        || basic.PerProcessUserTimeLimit > CPU_TICKS
    {
        return Err(denied());
    }
    Ok(())
}

pub(crate) struct Worker {
    process: OwnedHandle,
    job: OwnedHandle,
    _image: Option<tempfile::TempDir>,
    _profile: Profile,
    pub(crate) input: Option<File>,
    pub(crate) output: Option<File>,
    pub(crate) stderr: Option<File>,
}
impl Worker {
    pub(crate) fn spawn(executable: &Path, arguments: &[&str]) -> io::Result<Self> {
        let profile = Profile::new("lua")?;
        let image = tempfile::tempdir()?;
        image_permissions(image.path(), profile.sid)?;
        let copied = image.path().join("hedit-worker.exe");
        std::fs::copy(executable, &copied)?;
        // Do not rely on the source file's permissions or copy semantics.
        image_permissions(&copied, profile.sid)?;
        Self::launch(
            &copied,
            arguments,
            working_directory()?,
            profile,
            Some(image),
            native::Mode::Lua,
        )
    }
    pub(crate) fn spawn_native(launch: &crate::tool::Launch) -> io::Result<Self> {
        let profile = Profile::new("tool")?;
        image_permissions(&launch.executable, profile.sid)?;
        native::snapshot_permissions(&launch.snapshot, profile.sid)?;
        let arguments: Vec<_> = launch.arguments.iter().map(String::as_str).collect();
        Self::launch(
            &launch.executable,
            &arguments,
            wide(&launch.snapshot),
            profile,
            None,
            native::Mode::Tool,
        )
    }
    fn launch(
        copied: &Path,
        arguments: &[&str],
        current_directory: Vec<u16>,
        profile: Profile,
        image: Option<tempfile::TempDir>,
        mode: native::Mode,
    ) -> io::Result<Self> {
        let native = mode != native::Mode::Lua;
        let job = job(
            native || arguments == [crate::runtime::SERVICE_HELPER_ARG],
            native,
        )?;
        let asynchronous = mode == native::Mode::Tool;
        let (input, input_writer) = if asynchronous {
            let (parent, child) = connection(
                PIPE_ACCESS_OUTBOUND,
                FILE_FLAG_OVERLAPPED,
                FILE_READ_DATA | SYNCHRONIZE,
            )?;
            (child, parent)
        } else {
            pipe()?
        };
        let (output_reader, output) = if asynchronous {
            connection(
                PIPE_ACCESS_INBOUND,
                FILE_FLAG_OVERLAPPED,
                FILE_WRITE_DATA | SYNCHRONIZE,
            )?
        } else {
            pipe()?
        };
        for handle in [&input_writer, &output_reader] {
            check(unsafe { SetHandleInformation(handle.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) })?;
        }
        let security = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 1,
        };
        let (stderr_reader, stderr) = if asynchronous {
            let (read, write) = connection(
                PIPE_ACCESS_INBOUND,
                FILE_FLAG_OVERLAPPED,
                FILE_WRITE_DATA | SYNCHRONIZE,
            )?;
            check(unsafe { SetHandleInformation(read.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) })?;
            (Some(read), write)
        } else {
            (None, unsafe {
                owned(CreateFileW(
                    wide("NUL").as_ptr(),
                    GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    &security,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    ptr::null_mut(),
                ))
            }?)
        };
        let handles = [
            input.as_raw_handle(),
            output.as_raw_handle(),
            stderr.as_raw_handle(),
        ];
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid,
            Capabilities: ptr::null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        let child_policy = 1u32; // PROCESS_CREATION_CHILD_PROCESS_RESTRICTED
        let jobs = [job.as_raw_handle()];
        let mut attributes = Attributes::new(if native { 6 } else { 5 })?;
        let packages = 1u32; // PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT
        if native {
            attributes.set(
                PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
                &packages,
            )?;
        }
        attributes.set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, &child_policy)?;
        let mitigations = MITIGATIONS | if native { 1u64 << 28 } else { 0 };
        attributes.set(PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY, &mitigations)?;
        let startup = STARTUPINFOEXW {
            StartupInfo: STARTUPINFOW {
                cb: mem::size_of::<STARTUPINFOEXW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: input.as_raw_handle(),
                hStdOutput: output.as_raw_handle(),
                hStdError: stderr.as_raw_handle(),
                ..Default::default()
            },
            lpAttributeList: attributes.pointer(),
        };
        let mut command = native::command_line(copied, arguments)?;
        let environment = environment()?;
        let mut information = PROCESS_INFORMATION::default();
        // Console initialization can require a helper process and conflicts
        // with the child-process restriction. All worker I/O uses these pipes.
        let write_token = if mode == native::Mode::Tool || {
            #[cfg(test)]
            {
                mode == native::Mode::ToolProbe
            }
            #[cfg(not(test))]
            {
                false
            }
        } {
            Some(native::WriteToken::create()?)
        } else {
            None
        };
        let flags = CREATE_SUSPENDED
            | DETACHED_PROCESS
            | CREATE_UNICODE_ENVIRONMENT
            | EXTENDED_STARTUPINFO_PRESENT;
        let application = wide(copied);
        check(unsafe {
            if let Some(token) = &write_token {
                CreateProcessAsUserW(
                    token.token.as_raw_handle(),
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    1,
                    flags,
                    environment.as_ptr().cast(),
                    current_directory.as_ptr(),
                    &startup.StartupInfo,
                    &mut information,
                )
            } else {
                CreateProcessW(
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    ptr::null(),
                    ptr::null(),
                    1,
                    flags,
                    environment.as_ptr().cast(),
                    current_directory.as_ptr(),
                    &startup.StartupInfo,
                    &mut information,
                )
            }
        })
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("Confined process creation failed: {error}"),
            )
        })?;
        let process = unsafe { owned(information.hProcess) }?;
        let thread = unsafe { owned(information.hThread) }?;
        let worker = Self {
            process,
            job,
            _image: image,
            _profile: profile,
            input: Some(File::from(input_writer)),
            output: Some(File::from(output_reader)),
            stderr: stderr_reader.map(File::from),
        };
        // RAII owns the kill-on-close job before any verification can fail.
        require_token(worker.process.as_raw_handle(), Some(worker._profile.sid)).map_err(
            |error| {
                if native {
                    io::Error::new(
                        error.kind(),
                        format!("Native tool AppContainer verification failed: {error}"),
                    )
                } else {
                    error
                }
            },
        )?;
        if let Some(token) = &write_token {
            token.verify(worker.process.as_raw_handle())?;
        }
        if native {
            native::lower_integrity(worker.process.as_raw_handle(), mode)?;
            native::require_token(worker.process.as_raw_handle(), mode).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Native tool token verification failed: {error}"),
                )
            })?;
            native::require_job(worker.job.as_raw_handle()).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Native tool job verification failed: {error}"),
                )
            })?;
        }
        require_job(worker.job.as_raw_handle()).map_err(|error| {
            if native {
                io::Error::new(
                    error.kind(),
                    format!("Native tool process-limit verification failed: {error}"),
                )
            } else {
                error
            }
        })?;
        let mut included = 0;
        check(unsafe {
            IsProcessInJob(
                worker.process.as_raw_handle(),
                worker.job.as_raw_handle(),
                &mut included,
            )
        })?;
        if included == 0 {
            return Err(denied());
        }
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(worker)
    }
    pub(crate) fn over_thread_limit(&self) -> io::Result<bool> {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        let snapshot = unsafe { owned(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)) }?;
        let mut entry = THREADENTRY32 {
            dwSize: mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        check(unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) })?;
        let mut count = 0;
        let process = unsafe { GetProcessId(self.process.as_raw_handle()) };
        for _ in 0..65536 {
            if entry.th32OwnerProcessID == process {
                count += 1;
                if count > 32 {
                    return Ok(true);
                }
            }
            if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
                if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                    return Err(io::Error::last_os_error());
                }
                return Ok(false);
            }
        }
        Err(io::Error::other(
            "Tool thread accounting exceeded its limit.",
        ))
    }
    pub(crate) fn finished(&self, wait_ms: u32) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), wait_ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub(crate) fn over_cpu_limit(&self) -> io::Result<bool> {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        check(unsafe {
            GetProcessTimes(
                self.process.as_raw_handle(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        })?;
        let ticks = u64::from(user.dwHighDateTime) << 32 | u64::from(user.dwLowDateTime);
        Ok(ticks >= CPU_TICKS as u64)
    }
    pub(crate) fn success(&self) -> io::Result<bool> {
        Ok(self.exit_code()? == 0)
    }
    pub(crate) fn exit_code(&self) -> io::Result<u32> {
        let mut code = 0;
        check(unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) })?;
        Ok(code)
    }
    pub(crate) fn terminate(&self) {
        unsafe {
            TerminateJobObject(self.job.as_raw_handle(), 1);
            WaitForSingleObject(self.process.as_raw_handle(), 5000);
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub(crate) fn enter() -> io::Result<()> {
    require_token(unsafe { GetCurrentProcess() }, None)?;
    require_job(ptr::null_mut())?;
    let mut pipes = Vec::new();
    for standard in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE] {
        let handle = unsafe { GetStdHandle(standard) };
        if unsafe { GetFileType(handle) } != FILE_TYPE_PIPE {
            return Err(denied());
        }
        check(unsafe {
            GetNamedPipeInfo(
                handle,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        })?;
        pipes.push(handle);
    }
    if pipes[0] == pipes[1] {
        return Err(denied());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;

pub(crate) fn require_service_priority() -> io::Result<()> {
    let mut rate = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
    check(unsafe {
        QueryInformationJobObject(
            ptr::null_mut(),
            JobObjectCpuRateControlInformation,
            (&mut rate as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
            mem::size_of_val(&rate) as u32,
            ptr::null_mut(),
        )
    })?;
    if rate.ControlFlags
        & (JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP)
        != (JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP)
        || unsafe { rate.Anonymous.CpuRate } == 0
        || unsafe { rate.Anonymous.CpuRate } > 1000
    {
        return Err(denied());
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    check(unsafe {
        QueryInformationJobObject(
            ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            mem::size_of_val(&limits) as u32,
            ptr::null_mut(),
        )
    })?;
    if limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_PRIORITY_CLASS == 0
        || limits.BasicLimitInformation.PriorityClass != BELOW_NORMAL_PRIORITY_CLASS
    {
        return Err(denied());
    }
    Ok(())
}

mod native;
