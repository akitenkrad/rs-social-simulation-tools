use socsim_games::continuous::Cournot;
use socsim_games::{FictitiousPlay, GameSimFactory};

fn main() {
    let _ = GameSimFactory::new(Cournot::new(2, 100.0, 1.0, 10.0, 100.0).unwrap())
        .rule("fictitious_play", FictitiousPlay::new());
}
