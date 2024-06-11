use fixed::types::I16F16;
use fixed_macro::types::I16F16;

static POINTS: [I16F16; 10] = [
    I16F16!(0.0),
    I16F16!(0.111111),
    I16F16!(0.222222),
    I16F16!(0.333333),
    I16F16!(0.444444),
    I16F16!(0.555555),
    I16F16!(0.666666),
    I16F16!(0.777777),
    I16F16!(0.888888),
    I16F16!(0.999999),
];
static VOLTAGES: [I16F16; 10] = [
    I16F16!(3.0),
    I16F16!(3.25),
    I16F16!(3.45),
    I16F16!(3.57),
    I16F16!(3.68),
    I16F16!(3.77),
    I16F16!(3.85),
    I16F16!(3.97),
    I16F16!(4.05),
    I16F16!(4.15),
];

pub fn voltage_to_level(voltage: I16F16) -> I16F16 {
    let idx = VOLTAGES.binary_search(&voltage).map_or_else(|x| x, |x| x);

    if idx >= POINTS.len() - 1 {
        return I16F16!(1.0);
    }

    let (v_low, v_high) = (VOLTAGES[idx], VOLTAGES[idx + 1]);
    let (lvl_low, lvl_high) = (POINTS[idx], POINTS[idx + 1]);

    let i = (voltage - v_low) / (v_high - v_low);
    lvl_low + i * (lvl_high - lvl_low)
}
