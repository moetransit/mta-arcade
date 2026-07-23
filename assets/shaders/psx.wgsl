// PS1 vertex snap + audio-reactive breathing.
// Snap: quantize clip-space positions to a virtual low-res grid — the wobble
// you remember is consoles doing fixed-point transform math.
// Breathing: geometry inflates along its normals with the music's bass.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

struct PsxSettings {
    // x: bass, y: lowmid, z: highmid, w: treble (0..1, smoothed)
    bands: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> psx: PsxSettings;

const SNAP_GRID: vec2<f32> = vec2<f32>(213.0, 120.0); // half of 426x240
const BREATH_AMOUNT: f32 = 0.22;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    var world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);

    var world_normal = vec3<f32>(0.0, 1.0, 0.0);
#ifdef VERTEX_NORMALS
    world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
#endif

    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );

    // bass breathing: inflate along normals
    out.world_position = vec4<f32>(
        out.world_position.xyz + world_normal * psx.bands.x * BREATH_AMOUNT,
        1.0,
    );

    out.position = position_world_to_clip(out.world_position.xyz);

    // snap xy in NDC, preserving perspective (w)
    let w = out.position.w;
    let ndc = out.position.xy / w;
    let snapped = floor(ndc * SNAP_GRID + vec2<f32>(0.5)) / SNAP_GRID;
    out.position = vec4<f32>(snapped * w, out.position.z, w);

#ifdef VERTEX_NORMALS
    out.world_normal = world_normal;
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif

#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

    out.instance_index = vertex.instance_index;
    return out;
}
