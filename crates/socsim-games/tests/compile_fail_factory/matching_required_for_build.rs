use socsim_games::catalog::HawkDove;
use socsim_games::GameSimFactory;

fn main() {
    let factory = GameSimFactory::new(HawkDove::new(2.0, 4.0).unwrap())
        .agents(2)
        .history_len(1)
        .t_max(1);
    let _ = factory.build(7);
}
