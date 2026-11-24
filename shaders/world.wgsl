// Vorgegebener Welt-Shader: einfarbige Boxen, Material 3 leuchtet. Eigene Materialien bringt ein
// Spiel mit (siehe render::WorldShaders).

#include "_common.wgsl"
#include "_world_base.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let material = in.extra.x;
    let base = in.color.rgb;
    if (material > 2.5 && material < 3.5) {
        let lit = base * in.extra.z * flicker(in.extra.y, in.extra.w, g.cam_pos.w);
        return vec4<f32>(mix(lit, g.horizon.rgb, fog_amount(in.world_pos) * 0.4), 1.0);
    }
    return vec4<f32>(shade(base, in.world_pos, in.normal), 1.0);
}
