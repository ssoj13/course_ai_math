//! Artifact-bound real GPU trials; all checks evaluate captured pixels.
pub mod cosines;
pub mod cosines_input;
#[cfg(target_arch="wasm32")]
mod cosines_web;
#[cfg(target_arch="wasm32")]
mod editor_web;
use sim_render::{gpu::*,material::Material,capability::CapabilityReport};
use serde::Serialize;
use standard_surface_bsdf::SurfaceInputs;
#[cfg(not(target_arch="wasm32"))]
use standard_surface_bsdf::{ShadingFrame,Environment};
const WIDTH:u32=512;const HEIGHT:u32=256;
#[derive(Serialize)]
pub struct PixelCase { pub name:String,pub position:[u32;2],pub actual:[u8;4],pub expected_linear:[f32;3],pub max_error:f32,pub passed:bool }
#[derive(Serialize)]
pub struct PortableReport {pub suite:String,pub capability:CapabilityReport,pub cases:Vec<PixelCase>,pub edge_pixels:usize,pub interior_pixels:usize,pub passed:bool,pub device_groups:u32,pub allocated_bytes:u64,pub submissions:u32,pub shader_diagnostics:Vec<String>,pub shader_validation_backend:String}
fn swatches()->Vec<(&'static str,Material)>{
    let diffuse=SurfaceInputs{base_color:[0.35,0.6,0.45],specular:0.0,..SurfaceInputs::MATERIALX_DEFAULT};
    let plastic=SurfaceInputs{base_color:[0.3,0.35,0.65],specular_roughness:0.45,..SurfaceInputs::MATERIALX_DEFAULT};
    let metal=SurfaceInputs{base_color:[0.7,0.5,0.3],metalness:1.0,specular_roughness:0.6,..SurfaceInputs::MATERIALX_DEFAULT};
    let coat=SurfaceInputs{base_color:[0.6,0.3,0.3],coat:0.65,coat_roughness:0.4,specular_roughness:0.5,..SurfaceInputs::MATERIALX_DEFAULT};
    let emission=SurfaceInputs{base:0.0,specular:0.0,emission:0.5,emission_color:[0.3,0.8,0.9],..SurfaceInputs::MATERIALX_DEFAULT};
    [("diffuse",diffuse),("plastic",plastic),("metal",metal),("coat",coat),("emission",emission)].into_iter().map(|(name,surface)|(name,Material{surface,..Material::default()})).collect()
}
fn frame()->FrameGpu{FrameGpu{view_projection:glam::Mat4::IDENTITY.to_cols_array_2d(),eye:[0.0,0.0,10.0,1.0],
    orthographic_direction:[0.0,0.0,1.0,1.0],light_direction:[0.3,0.4,0.8660254,0.0],light_irradiance:[0.9,0.85,0.8,0.0],viewport:[WIDTH as f32,HEIGHT as f32,2.0,1.0]}}
fn env()->EnvironmentGpu{EnvironmentGpu{irradiance:[0.025,0.03,0.04,0.0],specular_radiance:[0.015,0.02,0.025,0.0],coat_radiance:[0.02,0.02,0.02,0.0],transmission_radiance:[0.0;4]}}
fn prepare(renderer:&RasterRenderer)->sim_render::Result<Vec<GpuDraw>>{
    swatches().iter().enumerate().map(|(j,(_,material))|{
        let x=-0.8+j as f32*0.4;
        let _=material;
        let vertices=[[-0.16,-0.6],[0.16,-0.6],[0.16,0.6],[-0.16,0.6]].map(|p|Vertex{position:[p[0]+x,p[1],0.5],normal:[0.0,0.0,1.0],tangent:[1.0,0.0,0.0]});
        renderer.prepare(&vertices,&[0,1,2,0,2,3],&frame(),&ObjectGpu::default(),material,&env(),true)
    }).collect()
}
fn target(device:&wgpu::Device,format:wgpu::TextureFormat,usage:wgpu::TextureUsages)->wgpu::Texture{
    device.create_texture(&wgpu::TextureDescriptor{label:Some("trial target"),size:wgpu::Extent3d{width:WIDTH,height:HEIGHT,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,usage,view_formats:&[]})
}
/// Execute production encoding and bounded asynchronous pixel readback.
pub async fn portable(device:&wgpu::Device,queue:&wgpu::Queue,capability:CapabilityReport)->Result<(PortableReport,Vec<u8>),String>{
    capability.validate_baseline().map_err(|e|e.to_string())?;
    let renderer=RasterRenderer::create(device,wgpu::TextureFormat::Rgba8Unorm,1).await.map_err(|e|e.to_string())?;
    let mut shader_diagnostics=Vec::new();
    let built_in=sim_render::shader::material_source();
    let invalid_sources=[
        ("syntax",String::from("@vertex fn broken {"),"widget_vs","widget_fs"),
        ("entry stage",built_in.clone(),"widget_fs","widget_vs"),
        ("foreign binding",built_in.replace("@group(0) @binding(0) var<uniform> widget_frame","@group(4) @binding(0) var<uniform> widget_frame"),"widget_vs","widget_fs"),
        ("resource type",built_in.replace("var<uniform> widget_frame","var<storage,read> widget_frame"),"widget_vs","widget_fs")
    ];
    for(name,source,vertex,fragment)in invalid_sources{
        match RasterRenderer::create_registered(device,wgpu::TextureFormat::Rgba8Unorm,1,&source,vertex,fragment).await {
            Ok(_)=>return Err(format!("invalid customshader{name} accepted")),
            Err(error)=>shader_diagnostics.push(format!("{name}: {error}")),
        }
    }
    // Existing valid renderer is retained through every failed candidate above.
    let draws=prepare(&renderer).map_err(|e|e.to_string())?;
    let color=target(device,wgpu::TextureFormat::Rgba8Unorm,wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC);
    let depth=target(device,wgpu::TextureFormat::Depth32Float,wgpu::TextureUsages::RENDER_ATTACHMENT);
    let readback=device.create_buffer(&wgpu::BufferDescriptor{label:Some("trial bounded pixel readback"),size:u64::from(WIDTH*HEIGHT*4),usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let scope=device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut encoder=device.create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("host owns submission")});
    renderer.encode_view(&mut encoder,&draws.iter().collect::<Vec<_>>(),&color.create_view(&Default::default()),&depth.create_view(&Default::default()),None,wgpu::Color{r:0.03,g:0.04,b:0.05,a:1.0},None).map_err(|e|e.to_string())?;
    encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo{texture:&color,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
        wgpu::TexelCopyBufferInfo{buffer:&readback,layout:wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(WIDTH*4),rows_per_image:Some(HEIGHT)}},wgpu::Extent3d{width:WIDTH,height:HEIGHT,depth_or_array_layers:1});
    let submission=queue.submit([encoder.finish()]);
    let (tx,rx)=futures_channel::oneshot::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read,move|r|{let _=tx.send(r);});
    #[cfg(not(target_arch="wasm32"))]
    device.poll(wgpu::PollType::Wait{submission_index:Some(submission),timeout:Some(std::time::Duration::from_secs(20))}).map_err(|e|e.to_string())?;
    #[cfg(target_arch="wasm32")]
    let _=submission;
    rx.await.map_err(|e|e.to_string())?.map_err(|e|e.to_string())?;
    let pixels=readback.slice(..).get_mapped_range().map_err(|e|e.to_string())?.to_vec();readback.unmap();
    if let Some(error)=scope.pop().await{return Err(error.to_string());}
    #[cfg(not(target_arch="wasm32"))]
    let environment=Environment{irradiance:[0.025,0.03,0.04],specular_radiance:[0.015,0.02,0.025],coat_radiance:[0.02;3],transmission_radiance:[0.0;3]};
    #[cfg(not(target_arch="wasm32"))]
    let frame_data=ShadingFrame{n:[0.0,0.0,1.0],tangent:[1.0,0.0,0.0],inside:false,curvature:0.0};
    let mut cases=Vec::new();
    for (j,(name,material)) in swatches().iter().enumerate(){
        let x=((0.1+j as f32*0.2)*WIDTH as f32) as u32;
        let y=100;
        let position=glam::Vec3::new(2.0*(x as f32+0.5)/WIDTH as f32-1.0,1.0-2.0*(y as f32+0.5)/HEIGHT as f32,0.5);
        let _=position;
        let wo=[0.0,0.0,1.0];
        let wi=glam::Vec3::new(0.3,0.4,0.8660254).normalize().to_array();
        #[cfg(not(target_arch="wasm32"))]
        let expected=material.evaluate(&frame_data,wo,wi,[0.9,0.85,0.8],&environment).map_err(|e|e.to_string())?;
        #[cfg(target_arch="wasm32")]
        let expected:[f32;3]={
            let _=(material,wo,wi);
            // Attested native public CPU BSDF receipt: task-03-portable-native.json,
            // unchanged materials, normal, orthographic view/light/environment above.
            [[1036239282,1042441503,1039145754],[1035479905,1036791171,1043415803],[1046537182,1041585284,1034300942],[1043306913,1035695025,1035450966],[1041865114,1053609165,1055286886]][j].map(f32::from_bits)
        };
        let offset=((y*WIDTH+x)*4) as usize;
        let actual=[pixels[offset],pixels[offset+1],pixels[offset+2],pixels[offset+3]];
        let max_error=(0..3).map(|k|(actual[k] as f32/255.0-expected[k].clamp(0.0,1.0)).abs()).fold(0.0,f32::max);
        cases.push(PixelCase{name:(*name).into(),position:[x,y],actual,expected_linear:expected,max_error,passed:max_error<0.025});
    }
    let edge_pixels=pixels.chunks_exact(4).filter(|p|p[0]>220 && p[1]>230 && p[2]>245).count();
    let interior_pixels=pixels.chunks_exact(4).filter(|p|p[0]>25 && p[0]<220).count();
    let passed=cases.iter().all(|c|c.passed)&&edge_pixels>200&&interior_pixels>10000;
    let report=PortableReport{suite:"portable".into(),capability,cases,edge_pixels,interior_pixels,passed,device_groups:device.limits().max_bind_groups,allocated_bytes:draws.iter().map(|d|d.allocated_bytes).sum(),submissions:1,shader_diagnostics,shader_validation_backend:if cfg!(target_arch="wasm32"){"Actual WebGPU module and fixed-interface pipelines with captured validation/internal/OOM"}else{"Native Naga AST plus actual wgpu pipelines"}.into()};
    Ok((report,pixels))
}
#[cfg(target_arch="wasm32")]
use wasm_bindgen::prelude::*;
/// Browser trial uses a real host-owned surface and separately captures production pixels.
#[cfg(target_arch="wasm32")]
#[wasm_bindgen]
pub async fn run_portable(canvas:web_sys::HtmlCanvasElement)->Result<String,JsValue>{
    console_error_panic_hook::set_once();
    canvas.set_width(WIDTH);canvas.set_height(HEIGHT);
    let instance=wgpu::Instance::default();
    let surface=instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas)).map_err(|e|JsValue::from_str(&e.to_string()))?;
    let adapter=instance.request_adapter(&wgpu::RequestAdapterOptions{compatible_surface:Some(&surface),..Default::default()}).await.map_err(|e|JsValue::from_str(&e.to_string()))?;
    let capability=CapabilityReport::inspect(&adapter);capability.validate_baseline().map_err(|e|JsValue::from_str(&e.to_string()))?;
    let (device,queue)=adapter.request_device(&wgpu::DeviceDescriptor::default()).await.map_err(|e|JsValue::from_str(&e.to_string()))?;
    let mut config=surface.get_default_config(&adapter,WIDTH,HEIGHT).ok_or_else(||JsValue::from_str("surface unsupported"))?;
    config.format=wgpu::TextureFormat::Bgra8Unorm;
    if !surface.get_capabilities(&adapter).formats.contains(&config.format){return Err(JsValue::from_str("linear Bgra8Unorm surface unavailable"));}
    surface.configure(&device,&config);
    let renderer=RasterRenderer::create(&device,config.format,1).await.map_err(|e|JsValue::from_str(&e.to_string()))?;
    let draws=prepare(&renderer).map_err(|e|JsValue::from_str(&e.to_string()))?;
    let depth=target(&device,wgpu::TextureFormat::Depth32Float,wgpu::TextureUsages::RENDER_ATTACHMENT);
    let output=match surface.get_current_texture(){wgpu::CurrentSurfaceTexture::Success(output)|wgpu::CurrentSurfaceTexture::Suboptimal(output)=>output,other=>return Err(JsValue::from_str(&format!("surface acquisition: {other:?}")))};
    let mut encoder=device.create_command_encoder(&Default::default());
    renderer.encode_view(&mut encoder,&draws.iter().collect::<Vec<_>>(),&output.texture.create_view(&Default::default()),&depth.create_view(&Default::default()),None,wgpu::Color{r:0.03,g:0.04,b:0.05,a:1.0},None).map_err(|e|JsValue::from_str(&e.to_string()))?;
    queue.submit([encoder.finish()]);queue.present(output);
    let (report,_)=portable(&device,&queue,capability).await.map_err(|e|JsValue::from_str(&e))?;
    device.destroy();
    serde_json::to_string(&report).map_err(|e|JsValue::from_str(&e.to_string()))
}
