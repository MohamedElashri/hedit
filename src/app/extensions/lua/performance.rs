// Opt-in CPU frame/allocation evidence; never a timing assertion in normal CI.
pub(in crate::app::extensions) mod measured_allocations {
    use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell,time::Instant};
    thread_local! {static COUNTS:Cell<Option<(u64,u64)>>=const{Cell::new(None)};}
    struct Allocator;
    #[global_allocator] static ALLOCATOR:Allocator=Allocator;
    fn record(size:usize) {let _=COUNTS.try_with(|state|if let Some((calls,bytes))=state.get() {state.set(Some((calls+1,bytes.saturating_add(size as u64))));});}
    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self,layout:Layout)->*mut u8 {let p=unsafe{System.alloc(layout)};if !p.is_null(){record(layout.size());}p}
        unsafe fn alloc_zeroed(&self,layout:Layout)->*mut u8 {let p=unsafe{System.alloc_zeroed(layout)};if !p.is_null(){record(layout.size());}p}
        unsafe fn realloc(&self,p:*mut u8,old:Layout,size:usize)->*mut u8 {let p=unsafe{System.realloc(p,old,size)};if !p.is_null(){record(size);}p}
        unsafe fn dealloc(&self,p:*mut u8,layout:Layout) {unsafe{System.dealloc(p,layout)}}
    }
    pub fn measure(call:impl FnOnce())->(u64,u64,u64) {
        struct Reset;impl Drop for Reset {fn drop(&mut self){COUNTS.with(|c|c.set(None));}}
        let _reset=Reset;COUNTS.with(|c|c.set(Some((0,0))));let start=Instant::now();call();let micros=start.elapsed().as_micros() as u64;
        let (calls,bytes)=COUNTS.with(|c|c.get().unwrap());(micros,calls,bytes)
    }
}
#[test]
#[ignore="controlled egui CPU frame/allocation probe; writes HEDIT_PLUGIN_UI_PROBE_OUT"]
fn profile_plugin_frames() {
    let output=std::env::var_os("HEDIT_PLUGIN_UI_PROBE_OUT").expect("Set HEDIT_PLUGIN_UI_PROBE_OUT");
    let mut results=Vec::new();
    for trial in 0..3 {
        for mode in if trial%2==0 {["disabled","installed_idle","stale_result_rejection"]}else{["stale_result_rejection","installed_idle","disabled"]} {
            let config=tempfile::tempdir().unwrap();
            let text="café / 東京\r\n".repeat(3000);
            let (_session,ctx,mut app)=sdk_app(config.path(),&text);
            if mode=="disabled" {app.extensions.clear();}
            for _ in 0..25 {crate::app::tests::frame(&ctx,&mut app,vec![]);}
            let mut samples=Vec::new();
            for index in 0..200 {
                let sample=measured_allocations::measure(|| {
                    if mode=="stale_result_rejection" && index%20==0 {
                        sdk_result(&mut app,vec![],None,false);
                    }
                    std::hint::black_box(crate::app::tests::frame(&ctx,&mut app,vec![egui::Event::Text("x".into())]));
                });
                samples.push(sample);
            }
            // Typing cancels the revision-bound synthetic result. This case
            // measures rejection during typing, never an in-process Lua path.
            assert!(app.active().page.text.len()>=text.len());
            results.push(serde_json::json!({"trial":trial,"mode":mode,"snapshot_bytes":text.len(),"samples_us_allocations_requested_bytes":samples}));
        }
    }
    let report=serde_json::json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"debug_assertions":cfg!(debug_assertions),"scope":"Instrumented egui CPU frames including input handling, layout, shapes, host result rejection, and per-thread allocation traffic. Excludes OS input delivery, GPU rendering/presentation, worker execution and other-thread allocations. Workloads use the same build, synthetic text, warmup and alternating case order. No text, paths, or key values in samples.","workloads":results});
    std::fs::write(output,serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}

#[test]
#[ignore="controlled provider CPU frames; requires HEDIT_PLUGIN_PROBE_EDITOR and HEDIT_PLUGIN_UI_PROBE_OUT"]
fn profile_provider_frames() {
    use std::time::{Duration,Instant};
    let editor=std::env::var_os("HEDIT_PLUGIN_PROBE_EDITOR").expect("Set HEDIT_PLUGIN_PROBE_EDITOR to the built editor");
    let output=std::env::var_os("HEDIT_PLUGIN_UI_PROBE_OUT").expect("Set HEDIT_PLUGIN_UI_PROBE_OUT");
    let helper=std::env::current_exe().unwrap().parent().unwrap().join("hedit-plugin.app");
    assert!(std::process::Command::new("python3").arg(concat!(env!("CARGO_MANIFEST_DIR"),"/scripts/macos_plugin_helper.py")).arg("--editor").arg(editor).arg("--output").arg(helper).status().unwrap().success());
    let text="café / 東京\r\n".repeat(2000);
    let mut results=Vec::new();
    for trial in 0..3 {
        let mut modes=vec!["disabled","installed_idle","typing_with_callbacks","disabled_paint","visible_diagnostics"];
        if trial%2==1 {modes.reverse();}
        for mode in modes {
            let config=tempfile::tempdir().unwrap();let directory=config.path().join("extensions");std::fs::create_dir(&directory).unwrap();
            for index in 0..4 {
                let id=format!("probe-{index}");let path=directory.join(format!("{id}.json"));
                let source=serde_json::json!({"api_version":4,"sdk_version":2,"id":id,"permissions":["document.read","events.observe","ui.present"],"commands":[{"id":"run","title":"Provider frame probe","provider":{"kind":"diagnostics","events":if mode=="visible_diagnostics"{vec!["open","manual"]}else{vec!["manual"]},"debounce_ms":50},"action":{"kind":"lua","source":"probe.lua"}}],"scripts":{"probe.lua":"return function(ctx) local items={} local start=0 for n=1,128 do items[n]={range={start=start,finish=start+5},severity='hint',message='Provider probe'} start=ctx.document.text:find('\\n',start+1,true) or #ctx.document.text end return {result={diagnostics=items}} end"}}).to_string();
                std::fs::write(&path,&source).unwrap();
                if !mode.starts_with("disabled") {catalog::set_lua_approved(config.path(),&path,&id,&format!("{id}.run"),&super::super::manifest::digest(&source),true).unwrap();}
            }
            let (_session,ctx,mut app)=crate::app::extensions::tests::app_with(&text,Some(config.path()));
            app.plugin_services.open=false;
            let until=Instant::now()+Duration::from_secs(5);
            for _ in 0..50 {crate::app::tests::frame(&ctx,&mut app,vec![]);std::thread::sleep(Duration::from_millis(5));}
            if mode=="visible_diagnostics" {
                while app.plugin_services.summary()!=Some(512) && Instant::now()<until {crate::app::tests::frame(&ctx,&mut app,vec![]);std::thread::sleep(Duration::from_millis(5));}
                assert_eq!(app.plugin_services.summary(),Some(512));
            }
            let mut samples=Vec::new();
            for index in 0..200 {
                let sample=measured_allocations::measure(|| {
                    if mode=="typing_with_callbacks" && index%10==0 {app.plugin_services.request(&format!("probe-{}.run",index/10%4));app.plugin_services.open=false;}
                    let events=if matches!(mode,"disabled_paint"|"visible_diagnostics"){vec![]}else{vec![egui::Event::Text("x".into())]};
                    std::hint::black_box(crate::app::tests::frame(&ctx,&mut app,events));
                });samples.push(sample);
            }
            if !matches!(mode,"disabled_paint"|"visible_diagnostics") {assert!(app.active().page.text.len()>=text.len()+200);}
            app.plugin_services.suspend();
            results.push(serde_json::json!({"trial":trial,"mode":mode,"snapshot_bytes":text.len(),"providers":4,"samples_us_allocations_requested_bytes":samples}));
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    std::fs::write(output,serde_json::to_vec_pretty(&serde_json::json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"debug_assertions":cfg!(debug_assertions),"scope":"Instrumented egui CPU input/layout/render-shape frames and per-thread allocation traffic, with real rotated native callbacks. Excludes OS input delivery, GPU presentation, worker-thread allocation and retained heap. Four approved packages, fixed synthetic text, warmup and alternating order; no document contents in samples.","workloads":results})).unwrap()).unwrap();
}
