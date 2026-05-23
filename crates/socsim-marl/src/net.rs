//! A burn-backed discrete-action policy network trained with REINFORCE.
//!
//! [`DiscretePolicyNet`] is a small two-layer MLP (`obs → hidden → logits`)
//! whose weights are **deterministically initialised from a [`SimRng`]** and
//! updated by burn's [`Adam`] optimiser.  Computation runs on the pure-Rust
//! [`NdArray`] backend (CPU), which burn evaluates deterministically, so two
//! trainings sharing the same seed produce identical results — preserving the
//! platform's bit-reproducibility guarantee (§10).
//!
//! The backend is fixed to `Autodiff<NdArray>` and kept private to this module,
//! so the [`Policy`] surface (flat `&[f32]` features, `usize` actions) leaks no
//! tensor types and the rest of the crate is backend-agnostic.

use burn::backend::ndarray::NdArrayDevice;
use burn::backend::{Autodiff, NdArray};
use burn::module::{Module, Param};
use burn::nn::Linear;
use burn::optim::adaptor::OptimizerAdaptor;
use burn::optim::{Adam, AdamConfig, GradientsParams, Optimizer};
use burn::tensor::backend::Backend;
use burn::tensor::{activation, Int, Tensor, TensorData};
use rand::Rng;
use socsim_core::{Result, SimRng};

use crate::policy::{Policy, Transition};

/// Training backend: reverse-mode autodiff over the pure-Rust CPU `NdArray`.
type Be = Autodiff<NdArray>;
/// Concrete device handle for [`Be`] (`NdArrayDevice::Cpu`).
type Dev = NdArrayDevice;
/// Stored optimiser type (Adam over the policy module).
type PolicyOptim = OptimizerAdaptor<Adam, PolicyModel<Be>, Be>;

/// Hyper-parameters for [`DiscretePolicyNet`].
#[derive(Clone, Copy, Debug)]
pub struct NetConfig {
    /// Observation feature dimension.
    pub obs_dim: usize,
    /// Hidden layer width.
    pub hidden: usize,
    /// Number of discrete actions.
    pub n_actions: usize,
    /// Adam learning rate.
    pub lr: f64,
    /// Discount factor for returns.
    pub gamma: f32,
}

impl NetConfig {
    /// Convenience constructor with sensible defaults (`hidden = 16`,
    /// `lr = 1e-2`, `gamma = 0.99`).
    pub fn new(obs_dim: usize, n_actions: usize) -> Self {
        Self {
            obs_dim,
            hidden: 16,
            n_actions,
            lr: 1e-2,
            gamma: 0.99,
        }
    }
}

/// Two-layer MLP policy module (`burn` Module).
#[derive(Module, Debug)]
pub struct PolicyModel<B: Backend> {
    fc1: Linear<B>,
    fc2: Linear<B>,
}

impl<B: Backend> PolicyModel<B> {
    /// `(batch, obs_dim) → (batch, n_actions)` logits with a ReLU hidden layer.
    fn forward(&self, x: Tensor<B, 2>) -> Tensor<B, 2> {
        let h = activation::relu(self.fc1.forward(x));
        self.fc2.forward(h)
    }
}

/// Build a [`Linear`] layer with Xavier-uniform weights drawn from `rng`
/// (zero bias).  burn's `Linear` stores its weight as `[d_in, d_out]`.
fn init_linear(d_in: usize, d_out: usize, rng: &mut SimRng, device: &Dev) -> Linear<Be> {
    let bound = (6.0 / (d_in + d_out) as f64).sqrt() as f32;
    let w: Vec<f32> = (0..d_in * d_out).map(|_| rng.gen_range(-bound..bound)).collect();
    let weight = Tensor::<Be, 1>::from_data(TensorData::from(w.as_slice()), device)
        .reshape([d_in, d_out]);
    let bias = Tensor::<Be, 1>::zeros([d_out], device);
    Linear {
        weight: Param::from_tensor(weight),
        bias: Some(Param::from_tensor(bias)),
    }
}

/// Two-layer softmax policy network with its optimiser.
pub struct DiscretePolicyNet {
    device: Dev,
    /// `Option` so [`Optimizer::step`] (which consumes and returns the module)
    /// can be applied from `&mut self` via take/replace.
    model: Option<PolicyModel<Be>>,
    optim: PolicyOptim,
    cfg: NetConfig,
}

impl DiscretePolicyNet {
    /// Build a network and initialise every parameter deterministically from
    /// `rng` (Xavier-uniform weights, zero biases).
    pub fn new(cfg: NetConfig, rng: &mut SimRng) -> Result<Self> {
        let device = Dev::default();
        let model = PolicyModel {
            fc1: init_linear(cfg.obs_dim, cfg.hidden, rng, &device),
            fc2: init_linear(cfg.hidden, cfg.n_actions, rng, &device),
        };
        let optim = AdamConfig::new().init::<Be, PolicyModel<Be>>();
        Ok(Self {
            device,
            model: Some(model),
            optim,
            cfg,
        })
    }

    /// Action probabilities for a single observation.
    fn probs(&self, obs: &[f32]) -> Vec<f32> {
        let model = self.model.as_ref().expect("model present");
        let x = Tensor::<Be, 1>::from_data(TensorData::from(obs), &self.device)
            .reshape([1, self.cfg.obs_dim]);
        let logits = model.forward(x);
        let probs = activation::softmax(logits, 1);
        probs
            .into_data()
            .into_vec::<f32>()
            .expect("f32 tensor data")
    }

    /// REINFORCE update over collected episodes.  Computes discounted returns
    /// per trajectory, standardises them as a variance-reducing baseline, and
    /// takes one Adam step on the policy-gradient loss.  Returns the loss.
    fn update_impl(&mut self, episodes: &[Vec<Transition>]) -> f32 {
        let mut obs_flat: Vec<f32> = Vec::new();
        let mut actions: Vec<i32> = Vec::new();
        let mut returns: Vec<f32> = Vec::new();

        for ep in episodes {
            // Discounted return G_t = r_t + γ·G_{t+1}, computed back to front.
            let mut g = 0.0f32;
            let mut ep_returns = vec![0.0f32; ep.len()];
            for t in (0..ep.len()).rev() {
                g = ep[t].reward + self.cfg.gamma * g;
                ep_returns[t] = g;
            }
            for (tr, ret) in ep.iter().zip(ep_returns) {
                obs_flat.extend_from_slice(&tr.obs);
                actions.push(tr.action as i32);
                returns.push(ret);
            }
        }

        let n = actions.len();
        if n == 0 {
            return 0.0;
        }

        // Standardise returns as a baseline (mean 0, unit variance).
        let mean = returns.iter().sum::<f32>() / n as f32;
        let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / n as f32;
        let std = var.sqrt().max(1e-6);
        let adv: Vec<f32> = returns.iter().map(|r| (r - mean) / std).collect();

        let device = &self.device;
        let obs_t = Tensor::<Be, 1>::from_data(TensorData::from(obs_flat.as_slice()), device)
            .reshape([n, self.cfg.obs_dim]);
        let actions_t = Tensor::<Be, 1, Int>::from_data(TensorData::from(actions.as_slice()), device)
            .reshape([n, 1]);
        let adv_t = Tensor::<Be, 1>::from_data(TensorData::from(adv.as_slice()), device);

        // Take ownership of the model so the optimiser can return an updated one.
        let model = self.model.take().expect("model present");
        let logits = model.forward(obs_t); // (n, A)
        let log_probs = activation::log_softmax(logits, 1); // (n, A)
        // log π(a_t | s_t): gather the chosen action's log-prob per row → (n,).
        let chosen = log_probs.gather(1, actions_t).reshape([n]);
        // Policy-gradient loss: −E[ log π(a|s) · A ].
        let loss = (chosen * adv_t).mean().neg();
        let loss_val: f32 = loss.clone().into_scalar();

        let grads = loss.backward();
        let grads = GradientsParams::from_grads(grads, &model);
        self.model = Some(self.optim.step(self.cfg.lr, model, grads));
        loss_val
    }
}

impl Policy for DiscretePolicyNet {
    fn act(&self, obs: &[f32]) -> usize {
        argmax(&self.probs(obs))
    }

    fn sample(&self, obs: &[f32], rng: &mut SimRng) -> usize {
        categorical(&self.probs(obs), rng)
    }

    fn update(&mut self, episodes: &[Vec<Transition>]) -> Result<f32> {
        Ok(self.update_impl(episodes))
    }

    fn obs_dim(&self) -> usize {
        self.cfg.obs_dim
    }

    fn n_actions(&self) -> usize {
        self.cfg.n_actions
    }
}

/// Index of the largest element (ties broken toward the lower index).
fn argmax(xs: &[f32]) -> usize {
    let mut best = 0usize;
    for (i, &x) in xs.iter().enumerate() {
        if x > xs[best] {
            best = i;
        }
    }
    best
}

/// Sample a category from a probability vector using `rng`.
fn categorical(probs: &[f32], rng: &mut SimRng) -> usize {
    let r: f32 = rng.gen_range(0.0..1.0);
    let mut acc = 0.0f32;
    for (i, &p) in probs.iter().enumerate() {
        acc += p;
        if r < acc {
            return i;
        }
    }
    probs.len().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argmax_picks_largest() {
        assert_eq!(argmax(&[0.1, 0.7, 0.2]), 1);
        assert_eq!(argmax(&[0.5, 0.5]), 0); // tie → lower index
    }

    #[test]
    fn net_init_is_deterministic_for_same_seed() {
        let cfg = NetConfig::new(3, 2);
        let mut a = SimRng::from_seed(7);
        let mut b = SimRng::from_seed(7);
        let na = DiscretePolicyNet::new(cfg, &mut a).unwrap();
        let nb = DiscretePolicyNet::new(cfg, &mut b).unwrap();
        let obs = [0.5, -0.3, 1.0];
        assert_eq!(na.probs(&obs), nb.probs(&obs));
    }

    #[test]
    fn categorical_is_in_range() {
        let mut rng = SimRng::from_seed(0);
        let probs = [0.25, 0.25, 0.5];
        for _ in 0..100 {
            let a = categorical(&probs, &mut rng);
            assert!(a < 3);
        }
    }
}
