//! Tests des Renderers ohne GPU: Gruppierung der Instanzen, Sonnen-Kamera, Projektion.

use super::lists::build_groups;
use super::*;
use glam::Vec3;

fn md(mesh: usize, shadow: bool) -> MeshDraw {
    let m = MeshDraw::new(mesh, Mat4::IDENTITY);
    if shadow {
        m
    } else {
        m.no_shadow()
    }
}

#[test]
fn draws_are_grouped_per_mesh_and_shadow_groups_stay_contiguous() {
    let draws = [
        md(2, true),
        md(1, true),
        md(2, false),
        md(2, true),
        md(9, true),
    ];
    // Mesh 9 gibt es nicht (nur 3 Modelle) und faellt weg
    let (order, groups, shadow) = build_groups(&draws, 3, 100);
    assert_eq!(order.len(), 4);
    assert_eq!(
        groups,
        vec![
            Group {
                mesh: 1,
                first_inst: 0,
                count: 1
            },
            Group {
                mesh: 2,
                first_inst: 1,
                count: 3
            },
        ]
    );
    // Schattenwerfer stehen je Modell vorn: zwei von Mesh 2, das dritte nicht
    assert_eq!(
        shadow,
        vec![
            Group {
                mesh: 1,
                first_inst: 0,
                count: 1
            },
            Group {
                mesh: 2,
                first_inst: 1,
                count: 2
            },
        ]
    );
    let (order, _, _) = build_groups(&draws, 3, 2);
    assert_eq!(order.len(), 2, "Obergrenze wird eingehalten");
}

#[test]
fn sun_camera_covers_the_shadow_area() {
    let center = Vec3::new(3.0, 0.0, 120.0);
    let vp = light_view_proj(Vec3::new(0.4, 1.0, -0.3), center, 30.0);
    for p in [
        center,
        center + Vec3::new(20.0, 0.0, 20.0),
        center + Vec3::new(-20.0, 8.0, -18.0),
    ] {
        let c = vp.project_point3(p);
        assert!(c.x.abs() <= 1.0 && c.y.abs() <= 1.0, "{p} ausserhalb: {c}");
        assert!((0.0..=1.0).contains(&c.z), "{p} Tiefe {}", c.z);
    }
    // Weiter weg von der Sonne = groessere Tiefe
    let near = vp
        .project_point3(center + Vec3::new(0.4, 1.0, -0.3) * 5.0)
        .z;
    assert!(near < vp.project_point3(center).z);
}

#[test]
fn perspective_puts_world_up_at_the_top_of_the_image() {
    let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
    let p = perspective(1.2, 16.0 / 9.0, 0.1, 100.0) * view;
    let up = p.project_point3(Vec3::new(0.0, 1.0, 5.0));
    // NDC y = +1 ist oben im Bild (naga dreht y wie WebGPU)
    assert!(up.y > 0.0, "oben landet unten: {up}");
}

#[test]
fn hidden_draws_leave_both_passes_contiguous() {
    use super::lists::{DrawLists, Visible};
    // Sichtbarkeit ist hier in x (Kamera) und y (Sonne) des Modells versteckt
    let at = |mesh, cam: bool, sun: bool| {
        let x = Vec3::new(cam as u8 as f32, sun as u8 as f32, 0.0);
        MeshDraw::new(mesh, Mat4::from_translation(x))
    };
    let draws = [
        at(0, true, false),
        at(0, false, true),
        at(0, false, false),
        at(0, true, true),
        at(1, false, false),
        at(1, true, true).no_shadow(),
    ];
    let mut l = DrawLists::default();
    let vis = |m: &MeshDraw| Visible {
        camera: m.model.w_axis.x > 0.5,
        shadow: m.model.w_axis.y > 0.5,
        second: false,
    };
    for _ in 0..2 {
        l.build_meshes(&draws, 2, 100, vis); // zweimal: Listen werden wiederverwendet
        let order: Vec<u32> = l.order.iter().map(|o| o.1).collect();
        assert_eq!(order, [1, 3, 0, 5]);
        let g = |mesh, first_inst, count| Group {
            mesh,
            first_inst,
            count,
        };
        assert_eq!(l.groups, [g(0, 1, 2), g(1, 3, 1)]);
        assert_eq!(l.shadow_groups, [g(0, 0, 2)]);
        assert_eq!((l.drawn, l.culled, l.dropped), (3, 3, 0));
    }
}

#[test]
fn msaa_falls_back_to_what_the_card_supports() {
    use super::quality::clamp_samples;
    use vk::SampleCountFlags as S;
    let card = S::TYPE_1 | S::TYPE_2 | S::TYPE_4;
    assert_eq!(clamp_samples(8, card), S::TYPE_4);
    assert_eq!(clamp_samples(3, card), S::TYPE_2);
    assert_eq!(clamp_samples(0, card), S::TYPE_1);
}

#[test]
fn second_camera_and_ghosts_get_their_own_lists() {
    use super::lists::{DrawLists, SkinKind, Visible};
    let mut l = DrawLists::default();
    // x = Kamera, y = zweite Kamera
    let at = |c: bool, s: bool| {
        MeshDraw::new(
            0,
            Mat4::from_translation(Vec3::new(c as u8 as f32, s as u8 as f32, 0.0)),
        )
    };
    let vis = |m: &MeshDraw| Visible {
        camera: m.model.w_axis.x > 0.5,
        shadow: false,
        second: m.model.w_axis.y > 0.5,
    };
    let draws = [at(false, true), at(true, true), at(true, false)];
    l.build_meshes(&draws, 1, 100, vis);
    let order: Vec<u32> = l.order.iter().map(|o| o.1).collect();
    assert_eq!(order, [2, 1, 0]);
    let g = |first_inst, count| Group {
        mesh: 0,
        first_inst,
        count,
    };
    assert_eq!(l.groups, [g(0, 2)]);
    assert_eq!(l.second_groups, [g(1, 2)]);
    l.build_ghosts(
        &[at(true, false), at(false, true), at(true, true)],
        1,
        100,
        vis,
    );
    assert_eq!(l.ghosts, [0, 2], "Reihenfolge bleibt");
    assert_eq!(l.ghost_groups, [g(3, 2)]);

    let all = Visible {
        camera: true,
        shadow: true,
        second: true,
    };
    let ghost = SkinDraw {
        ghost: true,
        ..Default::default()
    };
    let skins = [ghost, SkinDraw::default()];
    l.build_skins(&skins, &[SkinDraw::default()], 100, |_| all);
    let kinds: Vec<_> = l.skins.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [SkinKind::Solid, SkinKind::Ghost, SkinKind::SecondOnly]
    );
    let v: Vec<_> = l
        .skins
        .iter()
        .map(|s| (s.vis.camera, s.vis.shadow, s.vis.second))
        .collect();
    assert_eq!(
        v,
        [
            (true, true, true),
            (true, false, false),
            (false, false, true)
        ]
    );
    l.build_skins(&skins, &[], 1, |_| all);
    assert_eq!((l.skins.len(), l.dropped), (1, 1));
}

#[test]
fn higher_points_are_further_up_on_the_screen() {
    let eye = Vec3::new(0.0, 2.0, -5.0);
    let vp = perspective(1.0, 1.5, 0.1, 100.0) * Mat4::look_at_rh(eye, Vec3::Y * 2.0, Vec3::Y);
    let mid = project(&vp, Vec3::Y * 2.0).unwrap();
    assert!((mid.x - 0.5).abs() < 1e-5 && (mid.y - 0.5).abs() < 1e-5);
    assert!(
        project(&vp, Vec3::Y * 3.0).unwrap().y < 0.5,
        "oben = kleines y"
    );
    assert!(
        project(&vp, Vec3::new(0.0, 2.0, -9.0)).is_none(),
        "hinter der Kamera"
    );
}
