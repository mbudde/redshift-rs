use transition;

/**
 * Impl for any gamma adjustment method
 */
pub trait GammaMethod {
    fn restore(&self);
    
    fn set_temperature(&self, setting: &transition::ColorSetting);

    fn start(&mut self);
}

/**
 * Curse my Java genes! Next up is the ProblemFactory
 */
pub trait GammaMethodProvider {
    fn init(&self) -> Box<GammaMethod>;
}

pub struct DummyMethod;
impl GammaMethod for DummyMethod {
    fn restore(&self) {}
    
    fn set_temperature(&self, setting: &transition::ColorSetting) {
        println!("Temperature: {}", setting.temp);
    }

    fn start(&mut self) {
        println!("WARNING: Using dummy gamma method! Display will not affected by this gamma method.");
    }
}

impl GammaMethodProvider for DummyMethod {
    fn init(&self) -> Box<GammaMethod> {
        Box::new(DummyMethod) as Box<GammaMethod>
    }
}
