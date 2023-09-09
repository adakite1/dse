pub fn gain(decibels: f64) -> f64 {
    10.0_f64.powf(decibels / 20.0)
}

pub fn decibels(gain: f64) -> f64 {
    20.0 * gain.log10()
}

pub fn timecents_to_milliseconds(timecents: i16) -> i32 {
    (1000.0_f64 * 2.0_f64.powf(timecents as f64 / 1200.0_f64)).round() as i32
}

