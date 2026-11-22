// Opt-in CPU frame/allocation evidence; never a timing assertion in normal CI.
mod measured_allocations {
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
