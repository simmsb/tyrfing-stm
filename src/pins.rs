macro_rules! define_pin {
    ($name:ident, $p:ident) => {
        #[allow(unused)]
        macro_rules! $name {
            () => {
                ::embassy_stm32::peripherals::$p
            };
        }

        ::paste::paste! {
            #[allow(unused)]
            macro_rules! [<take_ $name>] {
                ($pins:expr) => {
                    $pins.$p
                }
            }

            #[allow(unused)]
            pub(crate) use [<take_ $name>];
        }

        #[allow(unused)]
        pub(crate) use $name;
    };
}

define_pin!(battery_sense, PA0);
define_pin!(hdr, PA2);

#[cfg(feature = "board_dev")]
define_pin!(en, PA3);
#[cfg(feature = "board_v0")]
define_pin!(en, PB5);

#[cfg(feature = "board_v1")]
define_pin!(opamp_en, PB5);
#[cfg(feature = "board_v1")]
define_pin!(boost_en, PB6);
#[cfg(feature = "board_v1")]
define_pin!(shunt_select, PC14);

define_pin!(dac, PA4);
define_pin!(aux_r, PA6);
define_pin!(aux_g, PA7);
define_pin!(aux_b, PB0);
define_pin!(button_led, PC15);
define_pin!(button, PA8);
