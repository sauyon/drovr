mod compress;
mod herdr;
mod phase;
mod run;

fn main() {
    println!("Hello, world!");
}

/// Shared env-var lock for tests that mutate `XDG_DATA_HOME`.
/// All test modules import this so they serialize on the same mutex.
#[cfg(test)]
pub(crate) mod test_util {
    use std::sync::Mutex;
    pub static ENV_LOCK: Mutex<()> = Mutex::new(());
}
