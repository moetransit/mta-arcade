//! Deterministic quake-style movement — the netplay twin of the tuned
//! bevy_ahoy feel (speed 17.5, accel 16Hz, friction 18Hz; playtested).
//! Pure `step` function over POD state: rollback snapshots it by clone,
//! peers re-derive identical trajectories from identical inputs.

use crate::arena::{Aabb, RAMP};

pub const DT: f32 = 1.0 / crate::TICK_HZ as f32;

pub const SPEED: f32 = 17.5;
pub const ACCEL_HZ: f32 = 16.0;
/// Quake-style air control: accelerate along wish only while the velocity
/// component in the wish direction is below this cap. Preserves and permits
/// gaining speed (strafe/surf kinetics) — never damps toward a target.
pub const AIR_WISH_CAP: f32 = 1.0;
pub const AIR_ACCEL_ADD: f32 = 35.0;
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

    // ground: exponential accel toward wish (dashdance feel, playtested);
    // air: additive quake accel — the engine of bhop and surf kinetics.
    // never damps toward a target in air, so speed above SPEED survives.
    if state.grounded {
        if has_input {
            let k = 1.0 - (-ACCEL_HZ * DT).exp();
            state.vel[0] += (wish[0] * SPEED - state.vel[0]) * k;
            state.vel[2] += (wish[1] * SPEED - state.vel[2]) * k;
        } else {
            let f = (-FRICTION_HZ * DT).exp();
            state.vel[0] *= f;
            state.vel[2] *= f;
        }
    } else if has_input {
        let cur = state.vel[0] * wish[0] + state.vel[2] * wish[1];
        if cur < AIR_WISH_CAP {
            let add = (AIR_WISH_CAP - cur).min(AIR_ACCEL_ADD * DT);
            state.vel[0] += wish[0] * add;
            state.vel[2] += wish[1] * add;
        }
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
    resolve_ramp(state);
    // the ramp can push positionally (incl. downward from its underside);
    // clean up any resulting arena overlap by minimal penetration
    for _ in 0..3 {
        if !depenetrate(state, arena) {
            break;
        }
    }
    // safety floor: never fall out of the world
    if state.pos[1] < -20.0 {
        state.pos = [0.0, 2.0, 0.0];
        state.vel = [0.0; 3];
    }
}

/// Collide two sample spheres (feet, head) against the oriented ramp box:
/// closest-point push-out along the surface normal, velocity deflected to
/// slide. Walkable if the touched face points mostly up.
fn resolve_ramp(state: &mut PlayerState) {
    const R: f32 = 0.42;
    let (s, c) = (RAMP.rot_z.sin(), RAMP.rot_z.cos());
    for h in [0.45_f32, 1.35] {
        let p = [state.pos[0], state.pos[1] + h, state.pos[2]];
        // world -> ramp local (rotate by -rot_z around z, about the center)
        let dx = p[0] - RAMP.center[0];
        let dy = p[1] - RAMP.center[1];
        let local = [c * dx + s * dy, -s * dx + c * dy, p[2] - RAMP.center[2]];
        let q = [
            local[0].clamp(-RAMP.half[0], RAMP.half[0]),
            local[1].clamp(-RAMP.half[1], RAMP.half[1]),
            local[2].clamp(-RAMP.half[2], RAMP.half[2]),
        ];
        let d = [local[0] - q[0], local[1] - q[1], local[2] - q[2]];
        let dist2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if !(1e-12..R * R).contains(&dist2) {
            continue;
        }
        let dist = dist2.sqrt();
        let nl = [d[0] / dist, d[1] / dist, d[2] / dist];
        // local -> world normal (rotate by +rot_z)
        let n = [c * nl[0] - s * nl[1], s * nl[0] + c * nl[1], nl[2]];
        let push = R - dist;
        for (p, ni) in state.pos.iter_mut().zip(&n) {
            *p += ni * push;
        }
        let vn = state.vel[0] * n[0] + state.vel[1] * n[1] + state.vel[2] * n[2];
        if vn < 0.0 {
            for (v, ni) in state.vel.iter_mut().zip(&n) {
                *v -= ni * vn;
            }
        }
        // steep faces are surf surfaces, not ground: no friction, gravity
        // slides you — jump on, carve, gain speed (source ramp feel)
        if n[1] > 0.95 {
            state.grounded = true;
        }
    }
}

/// Positional de-penetration vs the AABB arena: push out along the axis of
/// least overlap (velocity-sign-agnostic — for overlap we didn't integrate
/// into). Returns whether anything was resolved.
fn depenetrate(state: &mut PlayerState, arena: &[Aabb]) -> bool {
    let me = player_aabb(state.pos);
    for solid in arena {
        if !overlaps(&me, solid) {
            continue;
        }
        // (axis, signed push) with minimal magnitude
        let mut best = (0usize, f32::INFINITY);
        for axis in 0..3 {
            let push_pos = solid.1[axis] - me.0[axis]; // push +axis
            let push_neg = me.1[axis] - solid.0[axis]; // push -axis
            if push_pos < best.1.abs() {
                best = (axis, push_pos);
            }
            if push_neg < best.1.abs() {
                best = (axis, -push_neg);
            }
        }
        let (axis, push) = best;
        state.pos[axis] += push;
        // kill velocity into the face; ground if we were pushed upward
        if state.vel[axis] * push.signum() < 0.0 {
            state.vel[axis] = 0.0;
        }
        if axis == 1 && push > 0.0 {
            state.grounded = true;
        }
        return true;
    }
    false
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

    #[test]
    fn ramp_underside_blocks() {
        let arena = graybox();
        let mut p = PlayerState::spawn([10.0, 0.0, 8.0], 0.0);
        for _ in 0..30 {
            step(&mut p, &NetInput::default(), &arena);
        }
        // walk +x into the space under the slope for 4 seconds
        let input = NetInput {
            buttons: BTN_RIGHT,
            ..Default::default()
        };
        let mut min_y = f32::MAX;
        for _ in 0..600 {
            step(&mut p, &input, &arena);
            min_y = min_y.min(p.pos[1]);
        }
        // blocked by the underside; never through, never below the floor
        // (regression: underside push once shoved players out of the world)
        assert!(p.pos[0] < 20.0, "passed through ramp: pos {:?}", p.pos);
        assert!(min_y > -0.1, "pushed below the floor: min_y {min_y}");
    }

    #[test]
    fn ramp_surfs_you_down_and_off() {
        let arena = graybox();
        // drop onto the slope from above: surf surface (not ground) —
        // gravity slides you down and off the foot, never through anything
        let mut p = PlayerState::spawn([14.0, 8.0, 8.0], 0.0);
        let mut min_y = f32::MAX;
        for _ in 0..240 {
            step(&mut p, &NetInput::default(), &arena);
            min_y = min_y.min(p.pos[1]);
        }
        assert!(min_y > -0.1, "went below the floor: min_y {min_y}");
        assert!(
            p.pos[0] > 18.0 && p.pos[1] < 0.1,
            "should slide off the ramp foot onto the floor: pos {:?}",
            p.pos
        );
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

    const GOLDEN: u64 = 3825664799770014094;
}
