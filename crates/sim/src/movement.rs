//! Deterministic quake-style movement — the netplay twin of the tuned
//! bevy_ahoy feel (speed 17.5, accel 16Hz, friction 18Hz; playtested).
//! Pure `step` function over POD state: rollback snapshots it by clone,
//! peers re-derive identical trajectories from identical inputs.

use crate::arena::Aabb;

pub const DT: f32 = 1.0 / crate::TICK_HZ as f32;

pub const SPEED: f32 = 17.5;
pub const ACCEL_HZ: f32 = 16.0;
pub const AIR_ACCEL_HZ: f32 = 4.0;
pub const FRICTION_HZ: f32 = 18.0;
pub const GRAVITY: f32 = 29.0;
/// sqrt(2 * GRAVITY * jump_height 1.8)
pub const JUMP_SPEED: f32 = 10.219_589;

pub const PLAYER_HALF_XZ: f32 = 0.4;
pub const PLAYER_HEIGHT: f32 = 1.8;
pub const EYE_HEIGHT: f32 = 1.6;

pub const BTN_FWD: u8 = 1 << 0;
pub const BTN_BACK: u8 = 1 << 1;
pub const BTN_LEFT: u8 = 1 << 2;
pub const BTN_RIGHT: u8 = 1 << 3;
pub const BTN_JUMP: u8 = 1 << 4;
pub const BTN_FIRE: u8 = 1 << 5;

/// Wire input: 5 bytes/frame. Yaw/pitch as milliradians.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NetInput {
    pub buttons: u8,
    pub yaw_mrad: i16,
    pub pitch_mrad: i16,
}

impl NetInput {
    pub fn yaw(&self) -> f32 {
        self.yaw_mrad as f32 / 1000.0
    }
    pub fn pitch(&self) -> f32 {
        self.pitch_mrad as f32 / 1000.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub grounded: bool,
}

impl PlayerState {
    pub fn spawn(pos: [f32; 3], yaw: f32) -> Self {
        Self {
            pos,
            vel: [0.0; 3],
            yaw,
            pitch: 0.0,
            grounded: false,
        }
    }
}

/// Advance one tick. Pure: same state + input + arena → same result, always.
pub fn step(state: &mut PlayerState, input: &NetInput, arena: &[Aabb]) {
    state.yaw = input.yaw();
    state.pitch = input.pitch();

    // wish direction in the ground plane from yaw + buttons
    let (sin, cos) = (state.yaw.sin(), state.yaw.cos());
    let fwd = [-sin, -cos]; // -z forward at yaw 0
    let right = [cos, -sin];
    let mut wish = [0.0_f32, 0.0_f32];
    let b = input.buttons;
    if b & BTN_FWD != 0 {
        wish[0] += fwd[0];
        wish[1] += fwd[1];
    }
    if b & BTN_BACK != 0 {
        wish[0] -= fwd[0];
        wish[1] -= fwd[1];
    }
    if b & BTN_RIGHT != 0 {
        wish[0] += right[0];
        wish[1] += right[1];
    }
    if b & BTN_LEFT != 0 {
        wish[0] -= right[0];
        wish[1] -= right[1];
    }
    let len = (wish[0] * wish[0] + wish[1] * wish[1]).sqrt();
    let has_input = len > 1e-4;
    if has_input {
        wish[0] /= len;
        wish[1] /= len;
    }

    // exponential accel toward wish velocity; friction when grounded w/o input
    let accel_hz = if state.grounded {
        ACCEL_HZ
    } else {
        AIR_ACCEL_HZ
    };
    let k = 1.0 - (-accel_hz * DT).exp();
    if has_input {
        state.vel[0] += (wish[0] * SPEED - state.vel[0]) * k;
        state.vel[2] += (wish[1] * SPEED - state.vel[2]) * k;
    } else if state.grounded {
        let f = (-FRICTION_HZ * DT).exp();
        state.vel[0] *= f;
        state.vel[2] *= f;
    }

    if state.grounded && b & BTN_JUMP != 0 {
        state.vel[1] = JUMP_SPEED;
        state.grounded = false;
    }
    state.vel[1] -= GRAVITY * DT;

    // integrate + resolve, axis by axis (deterministic, order fixed x,y,z)
    state.grounded = false;
    for axis in [0usize, 1, 2] {
        state.pos[axis] += state.vel[axis] * DT;
        resolve_axis(state, axis, arena);
    }
    // safety floor: never fall out of the world
    if state.pos[1] < -20.0 {
        state.pos = [0.0, 2.0, 0.0];
        state.vel = [0.0; 3];
    }
}

fn player_aabb(pos: [f32; 3]) -> Aabb {
    (
        [pos[0] - PLAYER_HALF_XZ, pos[1], pos[2] - PLAYER_HALF_XZ],
        [
            pos[0] + PLAYER_HALF_XZ,
            pos[1] + PLAYER_HEIGHT,
            pos[2] + PLAYER_HALF_XZ,
        ],
    )
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    (0..3).all(|i| a.0[i] < b.1[i] && b.0[i] < a.1[i])
}

fn resolve_axis(state: &mut PlayerState, axis: usize, arena: &[Aabb]) {
    let me = player_aabb(state.pos);
    for solid in arena {
        if !overlaps(&me, solid) {
            continue;
        }
        if state.vel[axis] > 0.0 {
            // moving +axis: clamp our max face to their min face
            let push = me.1[axis] - solid.0[axis];
            state.pos[axis] -= push;
        } else {
            let push = solid.1[axis] - me.0[axis];
            state.pos[axis] += push;
            if axis == 1 {
                state.grounded = true;
            }
        }
        state.vel[axis] = 0.0;
        return resolve_axis(state, axis, arena); // re-check after push
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::graybox;

    #[test]
    fn lands_on_floor_and_stays() {
        let arena = graybox();
        let mut p = PlayerState::spawn([0.0, 3.0, 8.0], 0.0);
        for _ in 0..120 {
            step(&mut p, &NetInput::default(), &arena);
        }
        assert!(p.grounded);
        assert!((p.pos[1] - 0.0).abs() < 1e-3, "y = {}", p.pos[1]);
    }

    #[test]
    fn walls_contain() {
        let arena = graybox();
        let mut p = PlayerState::spawn([0.0, 1.0, 8.0], 0.0);
        // run forward (toward -z wall at -29.5) for 10 seconds
        let input = NetInput {
            buttons: BTN_FWD,
            ..Default::default()
        };
        for _ in 0..600 {
            step(&mut p, &input, &arena);
        }
        assert!(p.pos[2] > -29.51 + PLAYER_HALF_XZ - 1e-3);
        assert!(p.pos[2] < -25.0, "should have reached the wall");
    }

    #[test]
    fn jump_apex_close_to_design_height() {
        let arena = graybox();
        let mut p = PlayerState::spawn([0.0, 0.0, 8.0], 0.0);
        for _ in 0..30 {
            step(&mut p, &NetInput::default(), &arena);
        }
        let mut apex = 0.0_f32;
        let jump = NetInput {
            buttons: BTN_JUMP,
            ..Default::default()
        };
        for _ in 0..60 {
            step(&mut p, &jump, &arena);
            apex = apex.max(p.pos[1]);
        }
        assert!((1.5..2.1).contains(&apex), "apex = {apex}");
    }

    /// Golden trajectory hash: the movement twin of the judgment test.
    #[test]
    fn determinism_hash() {
        fn fnv1a(h: u64, bytes: &[u8]) -> u64 {
            bytes.iter().fold(h, |h, &b| {
                (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
            })
        }
        let run = || {
            let arena = graybox();
            let mut p = PlayerState::spawn([0.0, 3.0, 8.0], 0.0);
            let mut h = 0xcbf2_9ce4_8422_2325_u64;
            for i in 0..3600_u64 {
                let input = NetInput {
                    buttons: (i % 31) as u8 & 0b1_1111,
                    yaw_mrad: ((i * 37) % 6283) as i16 - 3141,
                    pitch_mrad: ((i * 13) % 2000) as i16 - 1000,
                };
                step(&mut p, &input, &arena);
                for v in p.pos.iter().chain(p.vel.iter()) {
                    h = fnv1a(h, &v.to_le_bytes());
                }
            }
            h
        };
        assert_eq!(run(), run());
        assert_eq!(run(), GOLDEN);
    }

    const GOLDEN: u64 = 10433130871032134380;
}
