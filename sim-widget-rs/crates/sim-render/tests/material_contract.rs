use sim_render::material::{Material, MaterialGpu, narrow};
#[test]
fn abi_maps_all_channels_and_policy_separately() {
    let mut m = Material::default();
    m.surface.base = 0.27;
    m.surface.metalness = 0.23;
    m.surface.specular_ior = 1.61;
    m.surface.thin_film_thickness = 320.0;
    m.surface.coat = 0.31;
    m.surface.coat_color = [0.72, 0.63, 0.54];
    m.surface.emission_color = [0.21, 0.32, 0.43];
    m.surface.subsurface_radius = [1.2, 2.3, 3.4];
    let packed = MaterialGpu::encode(&m).unwrap();
    let flat = bytemuck::cast_slice::<[f32; 4], f32>(&packed.slots);
    let domain = m.surface.domain();
    for (index, (_, expected, _)) in domain.iter().enumerate() {
        assert_eq!(flat[index].to_bits(), expected.to_bits(), "channel{index}");
    }
    assert_eq!(flat[49], m.surface.thin_film_energy.code() as f32);
    assert_eq!(&flat[50..], [1.0, 0.0]);
    assert_eq!(std::mem::size_of::<MaterialGpu>(), 208);
    assert_eq!(std::mem::align_of::<MaterialGpu>(), 16);
    assert_eq!(std::mem::offset_of!(MaterialGpu, slots), 0);
}
#[test]
fn invalid_transport_and_narrowing_are_addressed() {
    let mut m = Material::default();
    m.surface.transmission = 0.5;
    assert!(
        m.validate()
            .unwrap_err()
            .to_string()
            .contains("transmission")
    );
    m.surface.transmission = 0.0;
    m.surface.subsurface = 0.2;
    assert!(m.validate().unwrap_err().to_string().contains("subsurface"));
    m.surface.subsurface = 0.0;
    m.alpha = 0.0;
    assert!(m.validate().unwrap_err().to_string().contains("alpha"));
    assert!(narrow(f64::MAX, "position.x").is_err());
    assert!(narrow(f64::NAN, "position.x").is_err());
}
#[test]
fn multisample_resolve_is_counted_before_allocation() {
    assert_eq!(
        sim_render::capability::target_bytes(100, 100, 4, 4096, 0, 360000).unwrap(),
        360000
    );
    assert!(sim_render::capability::target_bytes(100, 100, 4, 4096, 0, 359999).is_err());
    assert!(
        sim_render::capability::target_bytes(u32::MAX, u32::MAX, 16, u32::MAX, 0, u64::MAX)
            .is_err()
    );
}
