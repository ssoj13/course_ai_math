#[cfg(not(target_arch="wasm32"))]
fn main(){
    if let Err(error)=pollster::block_on(run()){eprintln!("{error}");std::process::exit(1);}
}
#[cfg(target_arch="wasm32")]
fn main(){}
#[cfg(not(target_arch="wasm32"))]
async fn run()->Result<(),String>{
    let instance=wgpu::Instance::default();
    let adapter=instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await.map_err(|e|e.to_string())?;
    let capabilities=sim_render::capability::CapabilityReport::inspect(&adapter);
    capabilities.validate_baseline().map_err(|e|e.to_string())?;
    let(device,queue)=adapter.request_device(&wgpu::DeviceDescriptor::default()).await.map_err(|e|e.to_string())?;
    let(report,pixels)=sim_render_trial::portable(&device,&queue,capabilities).await?;
    let json=serde_json::to_string_pretty(&report).map_err(|e|e.to_string())?;
    println!("{json}");
    if let Some(path)=std::env::args().nth(1){
        std::fs::write(&path,&json).map_err(|e|e.to_string())?;
        let mut ppm=b"P6\n512 256\n255\n".to_vec();for p in pixels.chunks_exact(4){ppm.extend_from_slice(&p[..3]);}
        std::fs::write(format!("{path}.ppm"),ppm).map_err(|e|e.to_string())?;
    }
    device.destroy();
    if report.passed{Ok(())}else{Err("portable GPU pixel assertions failed".into())}
}
