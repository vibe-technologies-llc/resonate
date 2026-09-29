use std::collections::HashMap;

use resonate_library::LookupOp;
use serde::Deserialize;

use crate::{Client, Host, Result, commons};

const ENTITY_DATA: &str = "/wiki/Special:EntityData/";

pub(crate) fn pictured(client: &Client, url: &str) -> Result<Option<String>> {
    let Some(entity) = entity(url) else {
        tracing::debug!(url, "a picture is only looked up against wikidata.org");
        return Ok(None);
    };
    let asked = format!("{ENTITY_DATA}{entity}.json");
    let Some(held) = client.json::<EntityDoc>(Host::Wikidata, LookupOp::Portrait, &asked)? else {
        return Ok(None);
    };

    Ok(held
        .entities
        .get(&entity)
        .and_then(|held| held.claims.best_picture())
        .and_then(commons::scaled))
}

pub(crate) fn pictured_among(held: &EntityDoc) -> Option<String> {
    held.entities
        .values()
        .find_map(|held| held.claims.best_picture())
        .and_then(commons::scaled)
}

impl ClaimsDoc {
    fn best_picture(&self) -> Option<&str> {
        [Rank::Preferred, Rank::Normal]
            .into_iter()
            .find_map(|rank| {
                self.pictured
                    .iter()
                    .filter(|claim| claim.rank == rank)
                    .find_map(ClaimDoc::named)
            })
    }
}

impl ClaimDoc {
    fn named(&self) -> Option<&str> {
        Some(self.mainsnak.datavalue.as_ref()?.value.as_str())
    }
}

pub(crate) fn entity(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest
        .strip_prefix("www.wikidata.org")
        .or_else(|| rest.strip_prefix("wikidata.org"))?;
    let named = rest
        .strip_prefix("/wiki/")
        .or_else(|| rest.strip_prefix("/entity/"))?
        .split(['?', '#', '/'])
        .next()?;

    let mut letters = named.chars();
    let numbered = letters.next() == Some('Q') && letters.clone().count() > 0;

    (numbered && letters.all(|letter| letter.is_ascii_digit())).then(|| named.to_owned())
}

#[derive(Debug, Deserialize)]
pub(crate) struct EntityDoc {
    #[serde(default)]
    entities: HashMap<String, HeldDoc>,
}

#[derive(Debug, Deserialize)]
struct HeldDoc {
    #[serde(default)]
    claims: ClaimsDoc,
}

#[derive(Debug, Default, Deserialize)]
struct ClaimsDoc {
    #[serde(default, rename = "P18")]
    pictured: Vec<ClaimDoc>,
}

#[derive(Debug, Deserialize)]
struct ClaimDoc {
    mainsnak: SnakDoc,
    #[serde(default)]
    rank: Rank,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Rank {
    Preferred,
    Deprecated,
    #[default]
    #[serde(other)]
    Normal,
}

#[derive(Debug, Deserialize)]
struct SnakDoc {
    datavalue: Option<DataValueDoc>,
}

#[derive(Debug, Deserialize)]
struct DataValueDoc {
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wikidata_link_names_the_entity_it_points_at() {
        assert_eq!(
            entity("https://www.wikidata.org/wiki/Q2306").as_deref(),
            Some("Q2306")
        );
        assert_eq!(
            entity("http://wikidata.org/wiki/Q2306#sitelinks").as_deref(),
            Some("Q2306")
        );
        assert_eq!(
            entity("https://www.wikidata.org/entity/Q11649").as_deref(),
            Some("Q11649")
        );

        assert_eq!(entity("https://www.wikidata.org/wiki/Property:P18"), None);
        assert_eq!(entity("https://www.wikidata.org/wiki/Q"), None);
        assert_eq!(entity("https://en.wikipedia.org/wiki/Q2306"), None);
        assert_eq!(entity("not a url"), None);
    }

    #[test]
    fn an_entity_carrying_an_image_names_the_file_it_is_scaled_from() {
        let held: EntityDoc = serde_json::from_str(include_str!("../tests/fixtures/wikidata.json"))
            .expect("the captured entity reads back");
        let claims = held
            .entities
            .get("Q2306")
            .expect("the document names the entity it was asked for");

        assert_eq!(
            claims.claims.best_picture(),
            Some("PinkFloyd1973_retouched.jpg")
        );
    }

    fn claims(ranked: &[(&str, &str)]) -> ClaimsDoc {
        let pictured: Vec<serde_json::Value> = ranked
            .iter()
            .map(|(file, rank)| {
                serde_json::json!({
                    "mainsnak": { "datavalue": { "value": file, "type": "string" } },
                    "type": "statement",
                    "rank": rank,
                })
            })
            .collect();
        serde_json::from_value(serde_json::json!({ "P18": pictured }))
            .expect("the claims read back")
    }

    #[test]
    fn a_preferred_picture_is_taken_over_an_earlier_one_and_a_deprecated_one_never() {
        assert_eq!(
            claims(&[("Old.jpg", "normal"), ("Chosen.jpg", "preferred")]).best_picture(),
            Some("Chosen.jpg")
        );
        assert_eq!(
            claims(&[("Wrong.jpg", "deprecated"), ("Right.jpg", "normal")]).best_picture(),
            Some("Right.jpg")
        );
        assert_eq!(claims(&[("Wrong.jpg", "deprecated")]).best_picture(), None);
        assert_eq!(
            claims(&[("First.jpg", "normal"), ("Second.jpg", "normal")]).best_picture(),
            Some("First.jpg")
        );
    }
}
