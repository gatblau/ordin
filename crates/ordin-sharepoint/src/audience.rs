//! Who can see a destination (Internal Logic 5.2; C-42).

use std::collections::BTreeSet;

use crate::adapter::SharePointAdapter;
use crate::error::SharePointError;
use crate::graph::Req;
use crate::types::{Audience, LinkScope, SharePointLocation};
use crate::wire::{IdentitySet, ListPage, Permission};

/// C-42 fallback: the card names the site and says this; an automatic update cannot detect a grown audience.
const FALLBACK_LINE: &str = "everyone who can open it";

impl SharePointAdapter {
    /// Reads `permissions`. A 403, or a reply with no principals and no links, returns the C-42 fallback.
    // spec: ordin-desktop/components/SharePointAdapter#C-42
    pub async fn audience(&self, target: &SharePointLocation) -> Result<Audience, SharePointError> {
        let url = match &target.item_id {
            Some(id) => self.graph.url(
                &["drives", &target.drive_id, "items", id, "permissions"],
                &[],
            ),
            None => self
                .graph
                .url(&["drives", &target.drive_id, "root", "permissions"], &[]),
        };
        let reply = self
            .graph
            .call(Req::get("audience", url).pass(&[403]))
            .await?;
        if reply.status == 403 {
            return Ok(fallback());
        }
        let page: ListPage<Permission> = reply.json()?;
        Ok(summarise(&page.value))
    }
}

fn fallback() -> Audience {
    Audience {
        line: FALLBACK_LINE.to_owned(),
        people: None,
        everyone_who_can_open: true,
        principals: Vec::new(),
        link_scope: None,
    }
}

/// The principals, the broadest link scope and the one-line description of a permission list.
fn summarise(permissions: &[Permission]) -> Audience {
    let mut principals: BTreeSet<String> = BTreeSet::new();
    let mut groups: BTreeSet<String> = BTreeSet::new();
    let mut users: BTreeSet<String> = BTreeSet::new();
    let mut scope: Option<LinkScope> = None;
    for permission in permissions {
        let sets = permission
            .granted_to
            .iter()
            .chain(permission.granted_to_identities.iter());
        for set in sets {
            collect(set, &mut principals, &mut groups, &mut users);
        }
        if let Some(found) = permission
            .link
            .as_ref()
            .and_then(|l| l.scope.as_deref())
            .and_then(scope_of)
        {
            scope = Some(scope.map_or(found, |held| held.max(found)));
        }
    }
    if principals.is_empty() && scope.is_none() {
        return fallback();
    }
    let people = u32::try_from(users.len()).unwrap_or(u32::MAX);
    let group_names: Vec<&str> = groups.iter().take(2).map(String::as_str).collect();
    let line = match scope {
        Some(LinkScope::Anonymous) => "Anyone with the link".to_owned(),
        Some(LinkScope::Organization) => "Everyone in the company".to_owned(),
        _ => describe(&group_names, people),
    };
    Audience {
        line,
        // `people` is set only when no group is named and at least one individual user has access.
        people: (groups.is_empty() && people > 0).then_some(people),
        everyone_who_can_open: false,
        principals: principals.into_iter().collect(),
        link_scope: scope,
    }
}

/// "Procurement team and 3 people", "A and B and 1 person", "3 people", or just the group names.
fn describe(groups: &[&str], people: u32) -> String {
    let named = groups.join(" and ");
    let count = match people {
        0 => None,
        1 => Some("1 person".to_owned()),
        n => Some(format!("{n} people")),
    };
    match (named.is_empty(), count) {
        (true, Some(count)) => count,
        (true, None) => FALLBACK_LINE.to_owned(),
        (false, Some(count)) => format!("{named} and {count}"),
        (false, None) => named,
    }
}

fn scope_of(raw: &str) -> Option<LinkScope> {
    match raw {
        "anonymous" => Some(LinkScope::Anonymous),
        "organization" => Some(LinkScope::Organization),
        "users" => Some(LinkScope::Users),
        _ => None,
    }
}

/// Adds the ids of one grant: users, groups and site groups. Group display names name the audience; a
/// grant with no id still counts as a person or a named group.
fn collect(
    set: &IdentitySet,
    principals: &mut BTreeSet<String>,
    groups: &mut BTreeSet<String>,
    users: &mut BTreeSet<String>,
) {
    if let Some(user) = &set.user {
        if let Some(id) = &user.id {
            principals.insert(id.clone());
            users.insert(id.clone());
        }
    }
    for group in [&set.group, &set.site_group].into_iter().flatten() {
        if let Some(id) = &group.id {
            principals.insert(id.clone());
        }
        if let Some(name) = group.display_name.as_ref().or(group.id.as_ref()) {
            groups.insert(name.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permissions(json: serde_json::Value) -> Vec<Permission> {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_group_and_individuals_make_one_line() {
        let list = permissions(serde_json::json!([
            {"grantedToV2": {"siteGroup": {"id": "g1", "displayName": "Procurement team"}}},
            {"grantedToV2": {"user": {"id": "u1"}}},
            {"grantedToIdentitiesV2": [{"user": {"id": "u2"}}, {"user": {"id": "u3"}}]}
        ]));
        let audience = summarise(&list);
        assert_eq!(audience.line, "Procurement team and 3 people");
        assert_eq!(audience.principals, ["g1", "u1", "u2", "u3"]);
        assert!(!audience.everyone_who_can_open);
        assert_eq!(audience.people, None);
    }

    #[test]
    fn individuals_alone_are_counted() {
        let one = summarise(&permissions(serde_json::json!([
            {"grantedToV2": {"user": {"id": "u1"}}}
        ])));
        assert_eq!((one.line.as_str(), one.people), ("1 person", Some(1)));
        let two = summarise(&permissions(serde_json::json!([
            {"grantedToV2": {"user": {"id": "u1"}}},
            {"grantedToV2": {"user": {"id": "u2"}}}
        ])));
        assert_eq!((two.line.as_str(), two.people), ("2 people", Some(2)));
    }

    #[test]
    fn the_broadest_link_scope_names_the_audience() {
        let org = summarise(&permissions(serde_json::json!([
            {"link": {"scope": "users"}},
            {"link": {"scope": "organization"}}
        ])));
        assert_eq!(org.line, "Everyone in the company");
        assert_eq!(org.link_scope, Some(LinkScope::Organization));
        let anyone = summarise(&permissions(serde_json::json!([
            {"link": {"scope": "organization"}},
            {"link": {"scope": "anonymous"}, "grantedToV2": {"user": {"id": "u1"}}}
        ])));
        assert_eq!(anyone.line, "Anyone with the link");
        assert_eq!(anyone.link_scope, Some(LinkScope::Anonymous));
    }

    #[test]
    fn two_groups_are_joined_and_extra_ones_left_out() {
        let list = permissions(serde_json::json!([
            {"grantedToV2": {"group": {"id": "g1", "displayName": "Alpha"}}},
            {"grantedToV2": {"group": {"id": "g2", "displayName": "Beta"}}},
            {"grantedToV2": {"group": {"id": "g3", "displayName": "Gamma"}}}
        ]));
        assert_eq!(summarise(&list).line, "Alpha and Beta");
    }

    #[test]
    fn nothing_readable_falls_back() {
        let audience = summarise(&[]);
        assert_eq!(audience.line, "everyone who can open it");
        assert!(audience.everyone_who_can_open);
        assert!(audience.principals.is_empty());
        let unknown_scope = summarise(&permissions(
            serde_json::json!([{"link": {"scope": "other"}}]),
        ));
        assert!(unknown_scope.everyone_who_can_open);
        assert_eq!(describe(&[], 0), "everyone who can open it");
        assert_eq!(describe(&["A"], 0), "A");
    }
}
