fn boat_status(game: &Game<'_>, b: &mut Boat) -> Status {
    // AbstractBoat.getStatus at the pinned Horizon revision, with TerraForged's
    // MixinBoat: when isUnderwater reports a status and BoatHook.floatTheBoat lifts a
    // player's boat, the status is cleared and the checks go on.
    let bb = b.bounds();
    let (lo, hi) = (bb.min_position(), bb.max_position());
    let underwater = 'under: {
        let max_y = hi.y + 0.001;
        let mut under = false;
        for x in lo.x.floor() as i32..hi.x.ceil() as i32 {
            for y in hi.y.floor() as i32..max_y.ceil() as i32 {
                for z in lo.z.floor() as i32..hi.z.ceil() as i32 {
                    if let Some((h, source)) = game.fluid_height(x, y, z)
                        && max_y < f64::from(y) + f64::from(h)
                    {
                        if !source {
                            b.water_level = hi.y;
                            break 'under Some(Status::UnderFlowingWater);
                        }
                        under = true;
                    }
                }
            }
        }
        if under {
            b.water_level = hi.y;
            break 'under Some(Status::UnderWater);
        }
        None
    };
    if let Some(status) = underwater {
        if !terraforged_boat::float_the_boat(game, b) {
            return status;
        }
    }
    let bb = b.bounds();
    let (lo, hi) = (bb.min_position(), bb.max_position());
    // checkInWater
    let mut in_water = false;
    b.water_level = -f64::MAX;
    for x in lo.x.floor() as i32..hi.x.ceil() as i32 {
        for y in lo.y.floor() as i32..(lo.y + 0.001).ceil() as i32 {
            for z in lo.z.floor() as i32..hi.z.ceil() as i32 {
                if let Some((h, _)) = game.fluid_height(x, y, z) {
                    let level = f64::from(y) + f64::from(h);
                    b.water_level = b.water_level.max(level);
                    in_water |= lo.y < level;
                }
            }
        }
    }
    if in_water {
        return Status::InWater;
    }
    let friction = game.boat_ground_friction(bb);
    if friction > 0.0 {
        b.land_friction = friction;
        Status::OnLand
    } else {
        Status::InAir
    }
}
