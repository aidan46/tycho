use std::{any::Any, collections::HashMap, fmt, sync::Arc};

use async_trait::async_trait;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use tycho_common::{
    dto::ProtocolStateDelta,
    models::{protocol::GetAmountOutParams, token::Token},
    simulation::{
        errors::{SimulationError, TransitionError},
        indicatively_priced::{IndicativelyPriced, SignedQuote},
        protocol_sim::{Balances, GetAmountOutResult, ProtocolSim},
    },
    Bytes,
};

use crate::rfq::protocols::{
    hashflow::client::HashflowClient,
    maker_books::{MakerBook, MakerBooks, MakerVenue},
};

/// Hashflow's liquidity on one chain: every market maker's levels on every pair it quotes.
///
/// A swap takes its quote from one market maker and marks that maker used in the state it
/// returns. A maker's second quote does not account for its first fill, so by default a later
/// swap on that state goes to another maker.
#[derive(Clone, Serialize, Deserialize)]
pub struct HashflowVenueState {
    pub books: MakerBooks,
    pub client: Arc<HashflowClient>,
}

impl fmt::Debug for HashflowVenueState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HashflowVenueState")
            .field("books", &self.books)
            .finish_non_exhaustive()
    }
}

impl HashflowVenueState {
    pub fn new(
        books: Vec<MakerBook>,
        tokens: HashMap<Bytes, Token>,
        client: HashflowClient,
    ) -> Result<Self, SimulationError> {
        let rule = client.quote_rule();
        let books = MakerBooks::new(books, tokens, MakerVenue::Hashflow, rule)?;
        Ok(Self { books, client: Arc::new(client) })
    }
}

#[typetag::serde]
impl ProtocolSim for HashflowVenueState {
    fn fee(&self) -> f64 {
        todo!()
    }

    fn spot_price(&self, base: &Token, quote: &Token) -> Result<f64, SimulationError> {
        self.books
            .spot_price(&base.address, &quote.address)
    }

    fn get_amount_out(
        &self,
        amount_in: BigUint,
        token_in: &Token,
        token_out: &Token,
    ) -> Result<GetAmountOutResult, SimulationError> {
        let fill = self
            .books
            .best_fill(&amount_in, &token_in.address, &token_out.address)?;
        let new_state = Self {
            books: self
                .books
                .with_used(&fill.book.market_maker),
            client: self.client.clone(),
        };
        fill.result(151_000, Box::new(new_state))
    }

    fn get_limits(
        &self,
        sell_token: Bytes,
        buy_token: Bytes,
    ) -> Result<(BigUint, BigUint), SimulationError> {
        self.books
            .get_limits(&sell_token, &buy_token)
    }

    fn as_indicatively_priced(&self) -> Result<&dyn IndicativelyPriced, SimulationError> {
        Ok(self)
    }

    fn delta_transition(
        &mut self,
        _delta: ProtocolStateDelta,
        _tokens: &HashMap<Bytes, Token>,
        _balances: &Balances,
    ) -> Result<(), TransitionError> {
        todo!()
    }

    fn clone_box(&self) -> Box<dyn ProtocolSim> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn eq(&self, other: &dyn ProtocolSim) -> bool {
        let Some(other) = other
            .as_any()
            .downcast_ref::<HashflowVenueState>()
        else {
            return false;
        };
        self.books == other.books
    }
}

#[async_trait]
impl IndicativelyPriced for HashflowVenueState {
    /// Asks the market maker `get_amount_out` picks on this state for the same amount, and no
    /// other: a fallback maker could be one the route already fills against.
    ///
    /// Call it on the state the swap was simulated on, with the simulated amount. On the state
    /// that swap returned, its maker is used, so the request names another maker or fails.
    async fn request_signed_quote(
        &self,
        params: GetAmountOutParams,
    ) -> Result<SignedQuote, SimulationError> {
        let fill = self
            .books
            .best_fill(&params.amount_in, &params.token_in, &params.token_out)?;
        Ok(self
            .client
            .request_binding_quote_from_maker(&params, &fill.book.market_maker)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tokio::time::Duration;
    use tycho_common::models::Chain;

    use super::*;
    use crate::rfq::{
        models::{ComponentLayout, QuoteRule},
        protocols::test_utils::{
            maker_book, mock_quote_server, quote_params, usdc, wbtc, weth, weth_amount,
            HASHFLOW_QUOTE_RESPONSE,
        },
    };

    fn client(quote_rule: QuoteRule, quote_endpoint: String) -> HashflowClient {
        HashflowClient::new(
            Chain::Ethereum,
            HashSet::new(),
            0.0,
            HashSet::new(),
            "".to_string(),
            "".to_string(),
            Duration::from_secs(0),
            Duration::from_secs(1),
        )
        .unwrap()
        .with_component_layout(ComponentLayout::PerChain)
        .with_quote_rule(quote_rule)
        .with_quote_endpoint(quote_endpoint)
    }

    /// `test_mm_2` pays more for 0.5 WETH; `test_mm` also quotes WBTC.
    fn test_state(quote_rule: QuoteRule, quote_endpoint: String) -> HashflowVenueState {
        HashflowVenueState::new(
            vec![
                maker_book("test_mm", &weth(), &usdc(), &[(0.5, 3000.0), (1.5, 3000.0)]),
                maker_book("test_mm_2", &weth(), &usdc(), &[(0.5, 3010.0)]),
                maker_book("test_mm", &wbtc(), &usdc(), &[(1.0, 65000.0)]),
            ],
            HashMap::from([
                (weth().address, weth()),
                (usdc().address, usdc()),
                (wbtc().address, wbtc()),
            ]),
            client(quote_rule, quote_endpoint),
        )
        .unwrap()
    }

    #[test]
    fn get_amount_out_marks_the_maker_used() {
        let state = test_state(QuoteRule::OncePerMaker, String::new());
        let result = state
            .get_amount_out(weth_amount(0.5), &weth(), &usdc())
            .unwrap();
        assert_eq!(result.amount, BigUint::from(1_505_000_000u64));
        assert_eq!(result.gas, BigUint::from(151_000u64));
        let new_state = result
            .new_state
            .as_any()
            .downcast_ref::<HashflowVenueState>()
            .unwrap();
        assert!(new_state.books == state.books.with_used("test_mm_2"));
    }

    #[tokio::test]
    async fn request_signed_quote_names_the_picked_maker() {
        let (addr, request_log) = mock_quote_server(0, HASHFLOW_QUOTE_RESPONSE).await;
        // The mock quote is for 1 WETH -> WBTC, as `quote_params` asks.
        let state = HashflowVenueState::new(
            vec![
                maker_book("test_mm", &weth(), &wbtc(), &[(1.0, 0.05)]),
                maker_book("test_mm_2", &weth(), &wbtc(), &[(1.0, 0.051)]),
            ],
            HashMap::from([(weth().address, weth()), (wbtc().address, wbtc())]),
            client(QuoteRule::OncePerMaker, format!("http://127.0.0.1:{}/rfq", addr.port())),
        )
        .unwrap();
        state
            .request_signed_quote(quote_params())
            .await
            .unwrap();
        let after_first = HashflowVenueState {
            books: state.books.with_used("test_mm_2"),
            client: state.client.clone(),
        };
        after_first
            .request_signed_quote(quote_params())
            .await
            .unwrap();

        let requests = request_log.lock().unwrap();
        assert!(requests[0].contains("\"marketMakers\":[\"test_mm_2\"]"), "{}", requests[0]);
        assert!(requests[1].contains("\"marketMakers\":[\"test_mm\"]"), "{}", requests[1]);
    }
}
