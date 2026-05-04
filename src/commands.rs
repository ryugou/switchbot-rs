use crate::cli::BumpAxis;

pub const RGB_STEP: i32 = 16;
pub const BRIGHT_STEP: i32 = 10;
pub const TEMP_STEP: i32 = 100;

pub fn clamp(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max)
}

pub fn bump_rgb_channel(current: u8, delta: i32) -> u8 {
    clamp(current as i32 + delta, 0, 255) as u8
}

pub fn bump_brightness(current: u32, delta: i32) -> u32 {
    clamp(current as i32 + delta, 1, 100) as u32
}

pub fn bump_temperature(current: u32, delta: i32) -> u32 {
    clamp(current as i32 + delta, 2700, 6500) as u32
}

/// axis から (axis_kind, signed_step) を返す。
pub fn axis_delta(axis: BumpAxis) -> AxisDelta {
    use BumpAxis::*;
    match axis {
        RPlus => AxisDelta::Red(RGB_STEP),
        RMinus => AxisDelta::Red(-RGB_STEP),
        GPlus => AxisDelta::Green(RGB_STEP),
        GMinus => AxisDelta::Green(-RGB_STEP),
        BPlus => AxisDelta::Blue(RGB_STEP),
        BMinus => AxisDelta::Blue(-RGB_STEP),
        BrightPlus => AxisDelta::Brightness(BRIGHT_STEP),
        BrightMinus => AxisDelta::Brightness(-BRIGHT_STEP),
        TempPlus => AxisDelta::Temperature(TEMP_STEP),
        TempMinus => AxisDelta::Temperature(-TEMP_STEP),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisDelta {
    Red(i32),
    Green(i32),
    Blue(i32),
    Brightness(i32),
    Temperature(i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_basic() {
        assert_eq!(clamp(50, 0, 100), 50);
        assert_eq!(clamp(-5, 0, 100), 0);
        assert_eq!(clamp(150, 0, 100), 100);
        assert_eq!(clamp(0, 0, 100), 0);
        assert_eq!(clamp(100, 0, 100), 100);
    }

    #[test]
    fn bump_rgb_within_range() {
        assert_eq!(bump_rgb_channel(100, 16), 116);
        assert_eq!(bump_rgb_channel(100, -16), 84);
    }

    #[test]
    fn bump_rgb_clamps_at_max() {
        assert_eq!(bump_rgb_channel(250, 16), 255);
        assert_eq!(bump_rgb_channel(255, 16), 255);
    }

    #[test]
    fn bump_rgb_clamps_at_zero() {
        assert_eq!(bump_rgb_channel(10, -16), 0);
        assert_eq!(bump_rgb_channel(0, -16), 0);
    }

    #[test]
    fn bump_brightness_clamps_at_one() {
        assert_eq!(bump_brightness(5, -10), 1);
        assert_eq!(bump_brightness(1, -10), 1);
    }

    #[test]
    fn bump_brightness_clamps_at_100() {
        assert_eq!(bump_brightness(95, 10), 100);
        assert_eq!(bump_brightness(100, 10), 100);
    }

    #[test]
    fn bump_temperature_within_range() {
        assert_eq!(bump_temperature(3000, 100), 3100);
        assert_eq!(bump_temperature(3000, -100), 2900);
    }

    #[test]
    fn bump_temperature_clamps() {
        assert_eq!(bump_temperature(2750, -100), 2700);
        assert_eq!(bump_temperature(6450, 100), 6500);
        assert_eq!(bump_temperature(2700, -100), 2700);
        assert_eq!(bump_temperature(6500, 100), 6500);
    }

    #[test]
    fn axis_delta_mapping() {
        assert_eq!(axis_delta(BumpAxis::RPlus), AxisDelta::Red(16));
        assert_eq!(axis_delta(BumpAxis::RMinus), AxisDelta::Red(-16));
        assert_eq!(axis_delta(BumpAxis::BrightPlus), AxisDelta::Brightness(10));
        assert_eq!(
            axis_delta(BumpAxis::TempMinus),
            AxisDelta::Temperature(-100)
        );
    }
}
