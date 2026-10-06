use std::collections::HashMap;

use tycho_client::feed::synchronizer::ComponentWithState;
use tycho_common::{models::token::Token, Bytes};

use super::{client_builder::LiquoriceClientBuilder, venue_state::LiquoriceVenueState};
use crate::{
    protocol::{
        errors::InvalidSnapshotError,
        models::{DecoderContext, TryFromWithBlock},
    },
    rfq::{
        constants::get_liquorice_auth,
        models::{ComponentLayout, TimestampHeader},
        protocols::{
            component::{decode_venue, DecodedVenue},
            maker_books::MakerBook,
        },
    },
};

impl TryFromWithBlock<ComponentWithState, TimestampHeader> for LiquoriceVenueState {
    type Error = InvalidSnapshotError;

    async fn try_from_with_header(
        snapshot: ComponentWithState,
        _timestamp_header: TimestampHeader,
        _account_balances: &HashMap<Bytes, HashMap<Bytes, Bytes>>,
        all_tokens: &HashMap<Bytes, Token>,
        _decoder_context: &DecoderContext,
    ) -> Result<Self, Self::Error> {
        let DecodedVenue { books, tokens, quote_rule } =
            decode_venue::<MakerBook>(&snapshot, all_tokens)?;

        let auth = get_liquorice_auth().map_err(|e| {
            InvalidSnapshotError::ValueError(format!("Failed to get Liquorice authentication: {e}"))
        })?;
        let mut builder =
            LiquoriceClientBuilder::new(snapshot.component.chain, auth.solver, auth.key)
                .tokens(tokens.keys().cloned().collect())
                .component_layout(ComponentLayout::PerChain);
        if let Some(quote_rule) = quote_rule {
            builder = builder.quote_rule(quote_rule);
        }
        let client = builder.build().map_err(|e| {
            InvalidSnapshotError::MissingAttribute(format!("Couldn't create LiquoriceClient: {e}"))
        })?;

        LiquoriceVenueState::new(books, tokens, client)
            .map_err(|e| InvalidSnapshotError::ValueError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;
    use crate::rfq::protocols::test_utils::{decode, usdc, venue_snapshot, wbtc, weth};

    #[tokio::test]
    async fn test_decodes_books() {
        // Two makers on WBTC/USDC and one of them on WETH/USDC.
        env::set_var("LIQUORICE_USER", "test_solver");
        env::set_var("LIQUORICE_KEY", "test_key");
        let books = serde_json::json!([
            {
                "mm": "test_market_maker",
                "base_token": wbtc().address, "quote_token": usdc().address,
                "levels": [{ "q": "1.5", "p": "65000.0" }, { "q": "2.0", "p": "64950.0" }]
            },
            {
                "mm": "mm_b",
                "base_token": wbtc().address, "quote_token": usdc().address,
                "levels": [{ "q": "0.5", "p": "65100.0" }]
            },
            {
                "mm": "test_market_maker",
                "base_token": weth().address, "quote_token": usdc().address,
                "levels": [{ "q": "10", "p": "3000.0" }]
            }
        ]);
        let (snapshot, tokens) = venue_snapshot("rfq:liquorice", &[wbtc(), usdc(), weth()], &books);
        let state = decode::<LiquoriceVenueState>(snapshot, &tokens)
            .await
            .unwrap();

        let wbtc_books = state
            .books
            .pair_books(&wbtc().address, &usdc().address);
        assert_eq!(wbtc_books.len(), 2);
        assert_eq!(wbtc_books[0].market_maker, "mm_b");
        assert_eq!(wbtc_books[1].levels[0].quantity, 1.5);
        assert_eq!(wbtc_books[1].levels[0].price, 65000.0);
    }
}
