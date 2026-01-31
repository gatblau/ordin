//! Who can see a space or page (ConfluenceAdapter.md Internal Logic 8, C-42).

use crate::client::{next_relative, relative, Req};
use crate::error::ConfluenceError;
use crate::types::{Audience, ConfluenceLocation};
use crate::wire::{Listing, SpacePermission, WireRestriction};
use crate::ConfluenceAdapter;

/// The group that holds all licensed users.
const ALL_USERS_GROUP: &str = "confluence-users";

/// Step 8.2.
// spec: ordin-desktop/components/ConfluenceAdapter#C-42
fn fallback() -> Audience {
    Audience {
        line: "everyone who can open it".to_owned(),
        people: None,
        everyone_who_can_open: true,
        principals: Vec::new(),
    }
}

/// A reader of a kind this crate does not know. What such a grant covers is not known, so it is stated
/// as such and counted as nobody.
const NOT_KNOWN: &str = "Audience not known";
const OTHERS_NOT_KNOWN: &str = "others not known";

/// Up to two group names joined with " and ", plus "and N people" for individual users.
fn line_of(groups: &[String], users: usize) -> String {
    let named = groups
        .iter()
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join(" and ");
    let people = match users {
        1 => "1 person".to_owned(),
        n => format!("{n} people"),
    };
    match (named.is_empty(), users) {
        (true, _) => people,
        (false, 0) => named,
        (false, _) => format!("{named} and {people}"),
    }
}

fn audience_of(line: String, groups: usize, users: usize, mut principals: Vec<String>) -> Audience {
    principals.sort();
    principals.dedup();
    Audience {
        line,
        // Group sizes are not on the surface, so a count exists only when every reader is named.
        people: (groups == 0).then_some(users as u32),
        everyone_who_can_open: false,
        principals,
    }
}

impl ConfluenceAdapter {
    pub async fn audience(&self, loc: &ConfluenceLocation) -> Result<Audience, ConfluenceError> {
        match self.read_audience(loc).await {
            Err(ConfluenceError::AccessDenied) => Ok(fallback()),
            other => other,
        }
    }

    // spec: ordin-desktop/components/ConfluenceAdapter#C-42
    async fn read_audience(&self, loc: &ConfluenceLocation) -> Result<Audience, ConfluenceError> {
        // Without the scope the page's restrictions cannot be read, so the space answers: a restriction
        // only narrows it, and a card never says fewer people than the space lets in.
        let page = match &loc.page_id {
            Some(page_id) if self.transport.holds_restriction_scope().await? => Some(page_id),
            _ => None,
        };
        if let Some(page_id) = page {
            let url = relative(
                &[
                    "wiki",
                    "rest",
                    "api",
                    "content",
                    page_id,
                    "restriction",
                    "byOperation",
                    "read",
                ],
                &[],
            );
            let read: WireRestriction = self
                .transport
                .call(Req::get("page_restrictions", url))
                .await?
                .json()?;
            let users = read.restrictions.user.results;
            let groups = read.restrictions.group.results;
            if !users.is_empty() || !groups.is_empty() {
                let names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
                let mut principals: Vec<String> =
                    users.iter().map(|u| u.account_id.clone()).collect();
                principals.extend(
                    groups
                        .iter()
                        .map(|g| g.id.clone().unwrap_or_else(|| g.name.clone())),
                );
                return Ok(audience_of(
                    line_of(&names, users.len()),
                    names.len(),
                    users.len(),
                    principals,
                ));
            }
        }
        self.space_audience(&loc.space_key).await
    }

    async fn space_audience(&self, key: &str) -> Result<Audience, ConfluenceError> {
        let space = self.space_by_key(key).await?;
        let mut url = relative(
            &["wiki", "api", "v2", "spaces", &space.id, "permissions"],
            &[],
        );
        let mut grants = Vec::new();
        loop {
            let listing: Listing<SpacePermission> = self
                .transport
                .call(Req::get("space_permissions", url))
                .await?
                .json()?;
            grants.extend(listing.results);
            match listing.links.next {
                Some(next) => url = next_relative(&next)?,
                None => break,
            }
        }
        let readers: Vec<&SpacePermission> = grants
            .iter()
            .filter(|g| g.operation.key == "read" && g.operation.target_type == "space")
            .collect();
        if readers.is_empty() {
            return Ok(fallback());
        }
        let mut principals = Vec::new();
        let mut names = Vec::new();
        let mut users = 0;
        let mut anonymous = false;
        let mut everyone = false;
        let mut unknown = false;
        for reader in readers {
            let principal = &reader.principal;
            let id = principal
                .id
                .clone()
                .unwrap_or_else(|| principal.kind.clone());
            match principal.kind.as_str() {
                "anonymous" => anonymous = true,
                "group" => {
                    // The logic words the line in group names, and the surface states no
                    // name for a space grant; a reply without one shows the group's id.
                    let name = principal.name.clone().unwrap_or_else(|| id.clone());
                    everyone = everyone || name == ALL_USERS_GROUP || id == ALL_USERS_GROUP;
                    names.push(name);
                }
                "user" => users += 1,
                _ => unknown = true,
            }
            principals.push(id);
        }
        let line = if anonymous {
            "Anyone on the internet".to_owned()
        } else if everyone {
            "Everyone in the company".to_owned()
        } else if !unknown {
            line_of(&names, users)
        } else if names.is_empty() && users == 0 {
            NOT_KNOWN.to_owned()
        } else {
            format!("{} and {OTHERS_NOT_KNOWN}", line_of(&names, users))
        };
        let mut audience = audience_of(line, names.len(), users, principals);
        if unknown {
            audience.people = None;
        }
        Ok(audience)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_two_groups_and_the_people() {
        let groups = ["Finance".to_owned(), "Legal".to_owned(), "HR".to_owned()];
        assert_eq!(line_of(&groups, 3), "Finance and Legal and 3 people");
        assert_eq!(line_of(&groups[..1], 0), "Finance");
        assert_eq!(line_of(&[], 2), "2 people");
        assert_eq!(line_of(&[], 1), "1 person");
    }
}
