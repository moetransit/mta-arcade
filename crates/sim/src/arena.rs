//! Arena collision geometry as plain AABBs — the sim's single source of
//! truth for netplay. Mirrors the graybox in the game crate (which owns the
//! *visuals*); keep the two in sync until arenas load from shared data.
//!
//! v0 limitation: the surf ramp is visual-only in netplay (rotated boxes
//! need OBB or plane clipping — arrives with the real arena format).

/// (min, max) corners.
pub type Aabb = ([f32; 3], [f32; 3]);

fn boxed(center: [f32; 3], size: [f32; 3]) -> Aabb {
    (
        [
            center[0] - size[0] / 2.0,
            center[1] - size[1] / 2.0,
            center[2] - size[2] / 2.0,
        ],
        [
            center[0] + size[0] / 2.0,
            center[1] + size[1] / 2.0,
            center[2] + size[2] / 2.0,
        ],
    )
}

/// The graybox arena: floor, two platforms, six stair steps, four walls.
pub fn graybox() -> Vec<Aabb> {
    let mut boxes = vec![
        boxed([0.0, -0.5, 0.0], [60.0, 1.0, 60.0]),  // floor
        boxed([-8.0, 1.5, -6.0], [6.0, 1.0, 6.0]),   // platform 1
        boxed([-14.0, 3.0, -12.0], [6.0, 1.0, 6.0]), // platform 2
        boxed([0.0, 6.0, -30.0], [60.0, 12.0, 1.0]), // wall n
        boxed([0.0, 6.0, 30.0], [60.0, 12.0, 1.0]),  // wall s
        boxed([-30.0, 6.0, 0.0], [1.0, 12.0, 60.0]), // wall w
        boxed([30.0, 6.0, 0.0], [1.0, 12.0, 60.0]),  // wall e
    ];
    for i in 0..6 {
        boxes.push(boxed(
            [8.0, 0.15 + i as f32 * 0.3, -4.0 - i as f32],
            [3.0, 0.3, 1.0],
        ));
    }
    boxes
}

/// Spawn points, far apart, facing the middle.
/// (yaw 0 faces -z; the +z spawn needs yaw 0, the -z spawn needs yaw pi.)
pub const SPAWNS: [([f32; 3], f32); 2] = [
    ([0.0, 1.0, 8.0], 0.0),
    ([0.0, 1.0, -8.0], core::f32::consts::PI),
];
