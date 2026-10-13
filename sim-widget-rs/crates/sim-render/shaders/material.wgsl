// Owned wrapper appended to exact public wgsl_source() bytes.
struct WidgetFrame { view_projection:mat4x4<f32>, eye:vec4<f32>, orthographic_direction:vec4<f32>, light_direction:vec4<f32>, light_irradiance:vec4<f32>, viewport:vec4<f32> }
struct WidgetObject { world:mat4x4<f32>, normal_matrix:mat4x4<f32>, edge_color:vec4<f32> }
struct WidgetMaterial { slots:array<vec4<f32>,13> }
struct WidgetEnvironment { irradiance:vec4<f32>, specular_radiance:vec4<f32>, coat_radiance:vec4<f32>, transmission_radiance:vec4<f32> }
@group(0) @binding(0) var<uniform> widget_frame:WidgetFrame;
@group(1) @binding(0) var<uniform> widget_object:WidgetObject;
@group(2) @binding(0) var<uniform> widget_material:WidgetMaterial;
@group(3) @binding(0) var<uniform> widget_environment:WidgetEnvironment;
fn widget_inputs()->SsInputs {
    var i:SsInputs;
    i.base = widget_material.slots[0].x;
    i.base_color = vec3<f32>(widget_material.slots[0].y, widget_material.slots[0].z, widget_material.slots[0].w);
    i.diffuse_roughness = widget_material.slots[1].x;
    i.metalness = widget_material.slots[1].y;
    i.specular = widget_material.slots[1].z;
    i.specular_color = vec3<f32>(widget_material.slots[1].w, widget_material.slots[2].x, widget_material.slots[2].y);
    i.specular_roughness = widget_material.slots[2].z;
    i.specular_ior = widget_material.slots[2].w;
    i.specular_anisotropy = widget_material.slots[3].x;
    i.specular_rotation = widget_material.slots[3].y;
    i.transmission = widget_material.slots[3].z;
    i.transmission_color = vec3<f32>(widget_material.slots[3].w, widget_material.slots[4].x, widget_material.slots[4].y);
    i.transmission_extra_roughness = widget_material.slots[4].z;
    i.subsurface = widget_material.slots[4].w;
    i.subsurface_color = vec3<f32>(widget_material.slots[5].x, widget_material.slots[5].y, widget_material.slots[5].z);
    i.subsurface_radius = vec3<f32>(widget_material.slots[5].w, widget_material.slots[6].x, widget_material.slots[6].y);
    i.subsurface_scale = widget_material.slots[6].z;
    i.subsurface_anisotropy = widget_material.slots[6].w;
    i.sheen = widget_material.slots[7].x;
    i.sheen_color = vec3<f32>(widget_material.slots[7].y, widget_material.slots[7].z, widget_material.slots[7].w);
    i.sheen_roughness = widget_material.slots[8].x;
    i.coat = widget_material.slots[8].y;
    i.coat_color = vec3<f32>(widget_material.slots[8].z, widget_material.slots[8].w, widget_material.slots[9].x);
    i.coat_roughness = widget_material.slots[9].y;
    i.coat_anisotropy = widget_material.slots[9].z;
    i.coat_rotation = widget_material.slots[9].w;
    i.coat_ior = widget_material.slots[10].x;
    i.coat_affect_color = widget_material.slots[10].y;
    i.coat_affect_roughness = widget_material.slots[10].z;
    i.thin_film_thickness = widget_material.slots[10].w;
    i.thin_film_ior = widget_material.slots[11].x;
    i.emission = widget_material.slots[11].y;
    i.emission_color = vec3<f32>(widget_material.slots[11].z, widget_material.slots[11].w, widget_material.slots[12].x);
    i.thin_film_energy=u32(widget_material.slots[12].y);
    return i;
}
struct WidgetVertex { @builtin(position) clip:vec4<f32>, @location(0) world:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) tangent:vec3<f32> }
@vertex fn widget_vs(@location(0) position:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) tangent:vec3<f32>)->WidgetVertex{
    var out:WidgetVertex;
    let world=widget_object.world*vec4(position,1.0);
    out.clip=widget_frame.view_projection*world;
    out.world=world.xyz;
    out.normal=normalize((widget_object.normal_matrix*vec4(normal,0.0)).xyz);
    out.tangent=normalize((widget_object.world*vec4(tangent,0.0)).xyz);
    return out;
}
@fragment fn widget_fs(v:WidgetVertex)->@location(0) vec4<f32>{
    let i=widget_inputs();
    let f=SsFrame(normalize(v.normal),normalize(v.tangent),false,0.0);
    let wo=normalize(select(widget_frame.eye.xyz-v.world,widget_frame.orthographic_direction.xyz,widget_frame.orthographic_direction.w>0.5));
    let wi=normalize(widget_frame.light_direction.xyz);
    let direct=ss_eval_light_materialx(i,f,wo,wi);
    let env=SsEnvironment(widget_environment.irradiance.xyz,widget_environment.specular_radiance.xyz,widget_environment.coat_radiance.xyz,widget_environment.transmission_radiance.xyz);
    let indirect=ss_eval_environment(i,f,wo,env);
    let emission=ss_eval_emission(i,f,wo);
    let rgb=(direct.base+direct.specular+direct.transmission)*widget_frame.light_irradiance.xyz+indirect.base+indirect.specular+indirect.transmission+emission;
    let alpha=widget_material.slots[12].z;
    if alpha<widget_material.slots[12].w {discard;}
    return vec4(rgb,alpha);
}
struct WidgetStroke { @builtin(position) clip:vec4<f32>, @location(0) @interpolate(linear) local:vec2<f32>, @location(1) @interpolate(flat) extent:vec2<f32> }
@vertex fn widget_stroke_vs(@location(0) a:vec3<f32>,@location(1) b:vec3<f32>,@location(2) corner:vec2<f32>)->WidgetStroke{
    var out:WidgetStroke;
    var ca=widget_frame.view_projection*widget_object.world*vec4(a,1.0);
    var cb=widget_frame.view_projection*widget_object.world*vec4(b,1.0);
    let da=ca.w-ca.z; let db=cb.w-cb.z;
    out.clip=vec4(2.0,2.0,0.0,1.0);
    out.local=vec2(0.0);out.extent=vec2(0.0);
    if da<0.0 && db<0.0 {return out;}
    if da<0.0 {ca=mix(ca,cb,da/(da-db));}
    if db<0.0 {cb=mix(cb,ca,db/(db-(ca.w-ca.z)));}
    if ca.w<=0.000001 || cb.w<=0.000001 {return out;}
    let pa=ca.xy/ca.w;let pb=cb.xy/cb.w;
    let delta=(pb-pa)*widget_frame.viewport.xy*0.5;
    let length_px=length(delta);
    let direction=select(vec2(1.0,0.0),delta/max(length_px,0.000001),length_px>0.000001);
    let perpendicular=vec2(-direction.y,direction.x);
    let half_width=widget_frame.viewport.z*0.5;
    let fringe=1.0;
    let radius=half_width+fringe;
    let along=select(-radius,radius,corner.x>0.5);
    var c=mix(ca,cb,corner.x);
    let offset=direction*along+perpendicular*corner.y*radius;
    let clip_offset=offset*2.0/widget_frame.viewport.xy*c.w;
    c.x+=clip_offset.x;c.y+=clip_offset.y;
    c.z=min(c.w,c.z+0.00001*c.w);
    out.clip=c;
    out.local=vec2(corner.x*length_px+along,corner.y*radius);
    out.extent=vec2(length_px,half_width);
    return out;
}
@fragment fn widget_stroke_fs(v:WidgetStroke)->@location(0) vec4<f32>{
    // Exact screen-space capsule signed distance; includes circular caps and zero-length points.
    let nearest=vec2(clamp(v.local.x,0.0,v.extent.x),0.0);
    let distance_px=length(v.local-nearest)-v.extent.y;
    let coverage=clamp(0.5-distance_px,0.0,1.0);
    if coverage<=0.0 {discard;}
    return vec4(widget_object.edge_color.rgb,widget_object.edge_color.a*coverage);
}
