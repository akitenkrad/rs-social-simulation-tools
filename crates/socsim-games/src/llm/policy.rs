use std::collections::VecDeque;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use super::{
    ActionParser, DecisionRequest, Fallback, FallbackKind, GameRenderer, LlmDecisionRule, LlmReply,
    ObservationProvider, ParseOutcome, Resolution, ResolveContext, RetryProtocol,
};
use crate::{
    DecisionContext, ExperimentState, LearnContext, RuleError, RuleInitContext, Slot, StageGame,
};

/// The bounded record of recent LLM-backed decisions for one agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LlmConversation {
    pub max_turns: usize,
    pub turns: VecDeque<ConversationTurn>,
}

impl Default for LlmConversation {
    fn default() -> Self {
        Self {
            max_turns: 16,
            turns: VecDeque::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub t: u64,
    pub action: String,
    pub attempts: u32,
    pub gave_up: bool,
    pub payoff: Option<f64>,
}

pub struct LlmPolicy<G: StageGame, O, R, P> {
    obs: O,
    renderer: R,
    parser: P,
    retry: RetryProtocol<G::Action>,
    max_turns: usize,
    marker: PhantomData<fn() -> G>,
}

impl<G: StageGame, O, R, P> LlmPolicy<G, O, R, P> {
    pub fn new(obs: O, renderer: R, parser: P) -> Self {
        Self {
            obs,
            renderer,
            parser,
            retry: RetryProtocol::default(),
            max_turns: 16,
            marker: PhantomData,
        }
    }

    pub fn with_retry(mut self, retry: RetryProtocol<G::Action>) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_max_turns(mut self, n: usize) -> Self {
        self.max_turns = n;
        self
    }

    fn append_turn<S>(&self, state: &mut S, turn: ConversationTurn)
    where
        S: Slot<LlmConversation>,
    {
        let Some(conversation) = state.slot_mut() else {
            return;
        };
        conversation.turns.push_back(turn);
        while conversation.turns.len() > conversation.max_turns {
            conversation.turns.pop_front();
        }
    }
}

impl<G, X, O, R, P> LlmDecisionRule<G, X> for LlmPolicy<G, O, R, P>
where
    G: StageGame,
    X: ExperimentState,
    X::PolicyState: Slot<LlmConversation>,
    O: ObservationProvider<G, X>,
    R: GameRenderer<G, O::Obs>,
    P: ActionParser<G>,
{
    fn name(&self) -> &str {
        "llm_policy"
    }

    fn init_state(&self, _ctx: &RuleInitContext, game: &G) -> Result<X::PolicyState, RuleError> {
        self.parser.validate(game)?;
        Ok(X::PolicyState::from_slot(LlmConversation {
            max_turns: self.max_turns,
            turns: VecDeque::new(),
        }))
    }

    fn prepare(
        &self,
        state: &X::PolicyState,
        ctx: &DecisionContext<'_, G, X>,
    ) -> Result<DecisionRequest, RuleError> {
        if state.slot().is_none() {
            return Err(RuleError::MissingSlot("LlmConversation"));
        }
        let observation = self.obs.observe(ctx);
        Ok(DecisionRequest {
            system: Some(self.renderer.rules(ctx.game(), ctx.role())),
            prompt: self.renderer.turn(&observation, ctx.game()),
            attempt: 0,
        })
    }

    fn resolve(
        &self,
        state: &mut X::PolicyState,
        req: &DecisionRequest,
        reply: &LlmReply,
        rctx: &mut ResolveContext<'_, G>,
    ) -> Resolution<G::Action> {
        match self.parser.parse(&reply.text, rctx.game, rctx.role) {
            ParseOutcome::Ok { action, flags } => {
                self.append_turn(
                    state,
                    ConversationTurn {
                        t: rctx.t,
                        action: format!("{action:?}"),
                        attempts: req.attempt.saturating_add(1),
                        gave_up: false,
                        payoff: None,
                    },
                );
                Resolution::Accept { action, flags }
            }
            ParseOutcome::Violation(violation) if req.attempt < self.retry.max_retries => {
                let feedback = self.renderer.feedback(&violation);
                Resolution::Retry(DecisionRequest {
                    system: req.system.clone(),
                    prompt: format!("{}\n\n{feedback}", req.prompt),
                    attempt: req.attempt.saturating_add(1),
                })
            }
            ParseOutcome::Violation(violation) => {
                let (action, fallback) = match &self.retry.fallback {
                    Fallback::Previous => (rctx.previous.clone(), FallbackKind::Previous),
                    Fallback::UniformRandom => (
                        self.parser
                            .sample_uniform(rctx.game, rctx.role, rctx.fallback_rng),
                        FallbackKind::UniformRandom,
                    ),
                    Fallback::Specified(action) => (action.clone(), FallbackKind::Specified),
                };
                self.append_turn(
                    state,
                    ConversationTurn {
                        t: rctx.t,
                        action: format!("{action:?}"),
                        attempts: req.attempt.saturating_add(1),
                        gave_up: true,
                        payoff: None,
                    },
                );
                Resolution::GiveUp {
                    action,
                    fallback,
                    violation,
                }
            }
        }
    }

    fn learn_step(
        &self,
        state: &mut X::PolicyState,
        ctx: &LearnContext<'_, G, X>,
    ) -> Result<(), RuleError> {
        let conversation = state
            .slot_mut()
            .ok_or(RuleError::MissingSlot("LlmConversation"))?;
        if let Some(turn) = conversation
            .turns
            .back_mut()
            .filter(|turn| turn.t == ctx.t())
        {
            turn.payoff = Some(ctx.record().payoff);
        }
        Ok(())
    }
}
