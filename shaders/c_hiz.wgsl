// Tiefenpyramide: jede Stufe behaelt den fernsten Wert. Stufe 0 liest das Tiefenbild.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<r32float, write>;

@compute @workgroup_size(8, 8)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let from_size = textureDimensions(src);
    let a = id.xy * from_size / size;
    let b = ((id.xy + vec2<u32>(1u)) * from_size + size - vec2<u32>(1u)) / size;
    var far = 0.0;
    for (var y = a.y; y < min(b.y, from_size.y); y = y + 1u) {
        for (var x = a.x; x < min(b.x, from_size.x); x = x + 1u) {
            far = max(far, textureLoad(src, vec2<i32>(i32(x), i32(y)), 0).r);
        }
    }
    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(far, 0.0, 0.0, 0.0));
}
