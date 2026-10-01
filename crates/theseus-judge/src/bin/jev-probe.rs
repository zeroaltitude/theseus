//! `jev-probe`: real Jev calls on synthetic states, with the key from the
//! environment (`THESEUS_JEV_KEY`), never printed. The join moves it under
//! `theseus-sim jev-probe`.

fn main() -> anyhow::Result<()> {
    theseus_judge::probe::main()
}
