pub mod ledger;

pub fn run() -> i32 {
    let count: i32 = "three";
    ledger::total(&[1, 2]) + count
}
