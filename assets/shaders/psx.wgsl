// PS1 vertex snap: quantize clip-space positions to a virtual low-res grid.
// The wobble you remember is consoles doing fixed-point transform math;
// we fake it by snapping NDC coordinates to half-internal-resolution steps.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

const SNAP_GRID: vec2<f32> = vec2<f32>(213.0, 120.0); // half of 426x240

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    var world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);

    // snap xy in NDC, preserving perspective (w)
    let w = out.position.w;
    let ndc = out.position.xy / w;
    let snapped = floor(ndc * SNAP_GRID + vec2<f32>(0.5)) / SNAP_GRID;
    out.position = vec4<f32>(snapped * w, out.position.z, w);

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
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
