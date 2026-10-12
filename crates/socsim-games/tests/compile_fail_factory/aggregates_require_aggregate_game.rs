use socsim_games::catalog::PrisonersDilemma;
use socsim_games::GameSimFactory;

fn main() {
    let _ = GameSimFactory::new(PrisonersDilemma::new(5.0, 3.0, 1.0, 0.0).unwrap())
        .aggregates(4);
}
