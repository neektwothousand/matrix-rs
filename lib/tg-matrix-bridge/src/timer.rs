use std::time;

pub struct FunctionTimer<'a> {
	start: time::Instant,
	msg: &'a str,
}

impl<'a> FunctionTimer<'a> {
	pub fn new(msg: &'a str) -> Self {
		FunctionTimer {
			msg,
			start: time::Instant::now(),
		}
	}
}

macro_rules! timer {
	() => {
		let msg = format!("{}:{}", std::file!(), std::line!());
		let _ = crate::timer::FunctionTimer::new(&msg);
	};
}

impl Drop for FunctionTimer<'_> {
	fn drop(&mut self) {
		log::debug!("{} {:?}", self.msg, self.start.elapsed());
	}
}
