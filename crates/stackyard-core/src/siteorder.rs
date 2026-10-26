//! The order sites are listed in, and the groups the user puts them in (one
//! group per client, say). Kept in `site-order.toml` in the data home. Every
//! list of sites (the window, the tray, the command line, MCP) follows it.
//!
//! The file lists the top level in order; a group holds its own sites in
//! order. A site the file doesn't name (new, or parked since) goes last; a
//! name of a site that is gone is left out.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::settings::{read_toml, write_toml};
use crate::{Error, Result};

/// One entry of the top level: a site, or a group with its sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// The site's domain, for a site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    /// The group's name, for a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// A group's sites, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<String>,
    /// A group shown folded on the Sites screen.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
}

impl Item {
    pub fn site(domain: &str) -> Self {
        Item {
            site: Some(domain.to_string()),
            group: None,
            sites: Vec::new(),
            collapsed: false,
        }
    }

    pub fn group(name: &str, sites: &[&str]) -> Self {
        Item {
            site: None,
            group: Some(name.to_string()),
            sites: sites.iter().map(|s| s.to_string()).collect(),
            collapsed: false,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SiteOrder {
    #[serde(default, rename = "item")]
    items: Vec<Item>,
    #[serde(skip)]
    file: PathBuf,
}

pub fn file(home: &Path) -> PathBuf {
    home.join("site-order.toml")
}

impl SiteOrder {
    pub fn load(home: &Path) -> Result<Self> {
        let path = file(home);
        let mut order: SiteOrder = read_toml(&path)?.unwrap_or_default();
        order.file = path;
        Ok(order)
    }

    pub fn save(&self) -> Result<()> {
        write_toml(&self.file, self)
    }

    /// The groups and sites for these sites (in their usual order): every
    /// site once, unknown names left out, sites not placed yet at the end.
    pub fn arranged(&self, domains: &[String]) -> Vec<Item> {
        let known: HashSet<&str> = domains.iter().map(String::as_str).collect();
        let mut placed = HashSet::new();
        let mut take = |d: &String| known.contains(d.as_str()) && placed.insert(d.clone());
        let mut items = Vec::new();
        for item in &self.items {
            if let Some(name) = &item.group {
                let sites = item.sites.iter().filter(|d| take(d)).cloned().collect();
                items.push(Item {
                    site: None,
                    group: Some(name.clone()),
                    sites,
                    collapsed: item.collapsed,
                });
            } else if let Some(domain) = &item.site {
                if take(domain) {
                    items.push(Item::site(domain));
                }
            }
        }
        for domain in domains {
            if take(domain) {
                items.push(Item::site(domain));
            }
        }
        items
    }

    /// The sites' domains in the order they are listed, groups unfolded.
    pub fn sorted(&self, domains: &[String]) -> Vec<String> {
        self.arranged(domains)
            .into_iter()
            .flat_map(|i| match i.site {
                Some(d) => vec![d],
                None => i.sites,
            })
            .collect()
    }

    /// Puts `list` in this order, by each entry's domain.
    pub fn sort<T>(&self, list: &mut [T], domain: impl Fn(&T) -> String) {
        let domains: Vec<String> = list.iter().map(&domain).collect();
        let at: std::collections::HashMap<String, usize> = self
            .sorted(&domains)
            .into_iter()
            .enumerate()
            .map(|(i, d)| (d, i))
            .collect();
        list.sort_by_key(|x| at.get(&domain(x)).copied().unwrap_or(usize::MAX));
    }

    /// Replaces the order with the one the user arranged. Group names must
    /// be given and differ; an entry is a site or a group, not both.
    pub fn set(&mut self, items: Vec<Item>) -> Result<()> {
        let bad = |m: &str| Err(Error::SiteOrder(m.to_string()));
        let mut names = HashSet::new();
        let mut items = items;
        for item in &mut items {
            match (&mut item.group, &item.site) {
                (Some(name), None) => {
                    *name = name.trim().to_string();
                    if name.is_empty() {
                        return bad("a group needs a name");
                    }
                    if name.chars().count() > 60 {
                        return bad("a group name can be at most 60 characters");
                    }
                    if !names.insert(name.to_lowercase()) {
                        return bad(&format!("there is already a group named {name:?}"));
                    }
                }
                (None, Some(_)) => item.sites.clear(),
                _ => return bad("each entry must be either a site or a group"),
            }
        }
        self.items = items;
        Ok(())
    }

    /// A site's address changed: it keeps its place.
    pub fn rename(&mut self, from: &str, to: &str) {
        for item in &mut self.items {
            if item.site.as_deref() == Some(from) {
                item.site = Some(to.to_string());
            }
            for site in &mut item.sites {
                if site == from {
                    *site = to.to_string();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domains(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn new_sites_go_last_and_gone_ones_are_left_out() {
        let home = crate::testutil::tempdir();
        let mut order = SiteOrder::load(&home).unwrap();
        order
            .set(vec![
                Item::group("Acme", &["b.stack", "gone.stack"]),
                Item::site("c.stack"),
            ])
            .unwrap();
        order.save().unwrap();

        let order = SiteOrder::load(&home).unwrap();
        let all = domains(&["a.stack", "b.stack", "c.stack", "d.stack"]);
        assert_eq!(
            order.sorted(&all),
            domains(&["b.stack", "c.stack", "a.stack", "d.stack"])
        );
        let arranged = order.arranged(&all);
        assert_eq!(arranged[0], Item::group("Acme", &["b.stack"]));
        assert_eq!(arranged[1], Item::site("c.stack"));
        assert_eq!(arranged.len(), 4);
    }

    #[test]
    fn empty_groups_stay_and_sites_are_listed_once() {
        let mut order = SiteOrder::default();
        order
            .set(vec![
                Item::site("a.stack"),
                Item::group("Empty", &[]),
                Item::group("Two", &["a.stack", "b.stack"]),
            ])
            .unwrap();
        let arranged = order.arranged(&domains(&["a.stack", "b.stack"]));
        assert_eq!(
            arranged,
            vec![
                Item::site("a.stack"),
                Item::group("Empty", &[]),
                Item::group("Two", &["b.stack"]),
            ]
        );
    }

    #[test]
    fn sorts_any_list_by_domain() {
        let mut order = SiteOrder::default();
        order
            .set(vec![Item::site("z.stack"), Item::site("a.stack")])
            .unwrap();
        let mut list = vec![("a.stack", 1), ("m.stack", 2), ("z.stack", 3)];
        order.sort(&mut list, |x| x.0.to_string());
        assert_eq!(list, vec![("z.stack", 3), ("a.stack", 1), ("m.stack", 2)]);
    }

    #[test]
    fn group_names_must_be_given_and_differ() {
        let mut order = SiteOrder::default();
        assert!(order.set(vec![Item::group("  ", &[])]).is_err());
        assert!(order
            .set(vec![Item::group("Acme", &[]), Item::group("acme ", &[])])
            .is_err());
        let mut both = Item::site("a.stack");
        both.group = Some("Acme".into());
        assert!(order.set(vec![both]).is_err());
        order.set(vec![Item::group(" Acme ", &[])]).unwrap();
        assert_eq!(order.arranged(&[])[0].group.as_deref(), Some("Acme"));
    }

    #[test]
    fn a_renamed_site_keeps_its_place() {
        let mut order = SiteOrder::default();
        order
            .set(vec![
                Item::group("Acme", &["shop.stack"]),
                Item::site("blog.stack"),
            ])
            .unwrap();
        order.rename("shop.stack", "shop.test");
        assert_eq!(
            order.sorted(&domains(&["blog.stack", "shop.test"])),
            domains(&["shop.test", "blog.stack"])
        );
    }
}
