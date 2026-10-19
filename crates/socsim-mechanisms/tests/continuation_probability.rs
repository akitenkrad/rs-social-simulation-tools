use rand::Rng;
use socsim_core::{
    derive_seed, AgentId, Mechanism, Phase, Result, SimClock, SocsimError, StepContext, WorldState,
};
use socsim_engine::{Simulation, SimulationBuilder};
use socsim_log::InMemoryRecorder;
use socsim_mechanisms::ContinuationProbability;

#[derive(Clone, Debug)]
struct RandomHistoryWorld {
    clock: SimClock,
    draws: Vec<u64>,
}

impl RandomHistoryWorld {
    fn new(t_max: u64) -> Self {
        Self {
            clock: SimClock::new(t_max),
            draws: Vec::new(),
        }
    }
}

impl WorldState for RandomHistoryWorld {
    fn agent_ids(&self) -> Vec<AgentId> {
        Vec::new()
    }

    fn clock(&self) -> &SimClock {
        &self.clock
    }

    fn clock_mut(&mut self) -> &mut SimClock {
        &mut self.clock
    }
}

struct SharedStreamDraw;

impl Mechanism<RandomHistoryWorld> for SharedStreamDraw {
    fn name(&self) -> &str {
        "shared_stream_draw"
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Interaction]
    }

    fn apply(
        &mut self,
        _phase: Phase,
        ctx: &mut StepContext<'_, RandomHistoryWorld>,
    ) -> Result<()> {
        ctx.world.draws.push(ctx.rng.gen());
        Ok(())
    }
}

fn simulation(seed: u64, delta: Option<f64>, t_max: u64) -> Simulation<RandomHistoryWorld> {
    let mut builder = SimulationBuilder::new(RandomHistoryWorld::new(t_max))
        .seed(derive_seed(seed, &[1]))
        .add_mechanism(Box::new(SharedStreamDraw))
        .recorder(Box::new(InMemoryRecorder::new()));
    if let Some(delta) = delta {
        builder =
            builder.add_mechanism(Box::new(ContinuationProbability::new(delta, seed).unwrap()));
    }
    builder.build()
}

fn stop_event_t(simulation: &Simulation<RandomHistoryWorld>) -> Option<u64> {
    simulation
        .recorder()
        .as_any()
        .and_then(|recorder| recorder.downcast_ref::<InMemoryRecorder>())
        .unwrap()
        .events()
        .iter()
        .find(|event| event.kind == "continuation_stop")
        .map(|event| event.t)
}

#[test]
fn same_seed_stops_after_the_same_step() {
    let mut first = simulation(42, Some(0.9), 10_000);
    let mut second = simulation(42, Some(0.9), 10_000);

    first.run().unwrap();
    second.run().unwrap();

    assert_eq!(first.world().clock.t(), second.world().clock.t());
    assert_eq!(stop_event_t(&first), Some(first.world().clock.t()));
    assert_eq!(stop_event_t(&second), stop_event_t(&first));
}

#[test]
fn dedicated_stream_leaves_the_shared_rng_trajectory_unchanged() {
    let seed = 91;
    let mut shortened = simulation(seed, Some(0.8), 10_000);
    let mut full = simulation(seed, None, 10_000);

    shortened.run().unwrap();
    let stop_t = shortened.world().clock.t() as usize;
    for _ in 0..stop_t {
        full.step().unwrap();
    }

    assert_eq!(shortened.world().draws, full.world().draws);
}

#[test]
fn geometric_stop_time_has_the_expected_mean() {
    let n = 1_000_u64;
    let mean = (0..n)
        .map(|seed| {
            let mut run = simulation(seed, Some(0.9), 10_000);
            run.run().unwrap();
            run.world().clock.t() as f64
        })
        .sum::<f64>()
        / n as f64;

    // For a geometric distribution on {1, 2, ...}, variance is
    // delta / (1 - delta)^2 = 90. The standard error of this 1,000-seed
    // mean is sqrt(90 / 1,000) = 0.3, so 1.2 is a four-standard-error band.
    assert!((mean - 10.0).abs() <= 1.2, "observed mean {mean}");
}

#[test]
fn zero_stops_after_the_first_step_and_one_runs_to_t_max() {
    let mut zero = simulation(7, Some(0.0), 50);
    zero.run().unwrap();
    assert_eq!(zero.world().clock.t(), 1);
    assert_eq!(stop_event_t(&zero), Some(1));

    let mut one = simulation(7, Some(1.0), 50);
    one.run().unwrap();
    assert_eq!(one.world().clock.t(), 50);
    assert!(!one.stop_requested());
    assert_eq!(stop_event_t(&one), None);
}

#[test]
fn invalid_probabilities_are_configuration_errors() {
    for delta in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        match ContinuationProbability::new(delta, 0) {
            Err(SocsimError::Config(message)) => {
                assert!(message.contains("continuation probability"), "{message}");
                assert!(message.contains("[0, 1]"), "{message}");
            }
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(_) => panic!("accepted invalid continuation probability {delta}"),
        }
    }
}

#[test]
fn a_fresh_mechanism_resumes_its_private_stream_from_the_snapshot_clock() {
    let delta = 0.9;
    let (seed, stop_t) = (0..10_000)
        .find_map(|seed| {
            let mut run = simulation(seed, Some(delta), 200);
            run.run().unwrap();
            let stop_t = run.world().clock.t();
            (stop_t > 5 && stop_t < 200).then_some((seed, stop_t))
        })
        .expect("a seed whose geometric lifetime exceeds the checkpoint");

    let mut uninterrupted = simulation(seed, Some(delta), 200);
    uninterrupted.run().unwrap();

    let mut partial = simulation(seed, Some(delta), 200);
    for _ in 0..5 {
        partial.step().unwrap();
    }
    assert!(!partial.stop_requested());
    let snapshot = partial.snapshot();

    let mut resumed = simulation(seed, Some(delta), 200);
    resumed.restore(snapshot);
    resumed.run().unwrap();

    assert_eq!(resumed.world().clock.t(), stop_t);
    assert_eq!(resumed.world().clock.t(), uninterrupted.world().clock.t());
    assert_eq!(resumed.world().draws, uninterrupted.world().draws);
    assert_eq!(stop_event_t(&resumed), Some(stop_t));
}

#[test]
fn restoring_an_older_snapshot_rewinds_the_private_stream() {
    let delta = 0.95;
    let seed = (0..10_000)
        .find(|&seed| {
            let mut run = simulation(seed, Some(delta), 100);
            for _ in 0..5 {
                run.step().unwrap();
                if run.stop_requested() {
                    return false;
                }
            }
            run.run().unwrap();
            run.world().clock.t() > 10
        })
        .expect("a seed that runs past both checkpoints");

    let mut reference = simulation(seed, Some(delta), 100);
    for _ in 0..5 {
        reference.step().unwrap();
    }
    let snapshot = reference.snapshot();
    reference.run().unwrap();
    let expected = reference.world().clone();

    let mut rewound = simulation(seed, Some(delta), 100);
    for _ in 0..10 {
        rewound.step().unwrap();
    }
    rewound.restore(snapshot);
    rewound.run().unwrap();

    assert_eq!(rewound.world().clock.t(), expected.clock.t());
    assert_eq!(rewound.world().draws, expected.draws);
}
