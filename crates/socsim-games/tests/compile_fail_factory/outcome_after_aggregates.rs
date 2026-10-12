use socsim_games::catalog::MinorityGame;
use socsim_games::{GameSimFactory, NormalFormOutcome, WholePopulation};

fn main() {
    // The play path is chosen first and wrapped with aggregates last; once
    // wrapped, it can no longer be replaced.
    let _ = GameSimFactory::new(MinorityGame::new(5).unwrap())
        .matching(WholePopulation::new())
        .aggregates(4)
        .outcome(NormalFormOutcome);
    let _ = GameSimFactory::new(MinorityGame::new(5).unwrap())
        .matching(WholePopulation::new())
        .aggregates(4)
        .counting_play();
}
