use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::catalog::models::InstallMethod;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub package: String,
    pub method: InstallMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolverDecision {
    pub exact: Vec<Candidate>,
    pub prefix: Vec<Candidate>,
    pub contains: Vec<Candidate>,
}

impl ResolverDecision {
    pub fn auto_candidate(&self) -> Option<&Candidate> {
        if self.exact.len() == 1 {
            return self.exact.first();
        }
        if self.exact.is_empty() && self.prefix.len() == 1 {
            return self.prefix.first();
        }
        if self.exact.is_empty() && self.prefix.is_empty() && self.contains.len() == 1 {
            return self.contains.first();
        }
        None
    }

    pub fn requires_user_selection(&self) -> bool {
        self.auto_candidate().is_none()
            && (!self.exact.is_empty() || !self.prefix.is_empty() || !self.contains.is_empty())
    }
}

pub trait PackageSearch {
    fn search(&self, hint: &str, method: &InstallMethod) -> Result<Vec<String>>;
}

pub fn resolve_with_fallback(
    hint: &str,
    method: InstallMethod,
    initial_candidates: &[Candidate],
    search: &dyn PackageSearch,
) -> Result<ResolverDecision> {
    let initial = rank_candidates(hint, initial_candidates);
    if !initial.exact.is_empty() {
        return Ok(initial);
    }

    let searched = search
        .search(hint, &method)?
        .into_iter()
        .map(|package| Candidate {
            package,
            method: method.clone(),
        })
        .collect::<Vec<_>>();

    let mut merged = initial_candidates.to_vec();
    for candidate in searched {
        if !merged
            .iter()
            .any(|existing| existing.package == candidate.package)
        {
            merged.push(candidate);
        }
    }

    Ok(rank_candidates(hint, &merged))
}

pub fn rank_candidates(hint: &str, candidates: &[Candidate]) -> ResolverDecision {
    let mut exact = Vec::new();
    let mut prefix = Vec::new();
    let mut contains = Vec::new();

    let hint_lc = hint.to_ascii_lowercase();
    for candidate in candidates {
        let package = candidate.package.to_ascii_lowercase();
        if package == hint_lc {
            exact.push(candidate.clone());
            continue;
        }
        if package.starts_with(&hint_lc) {
            prefix.push(candidate.clone());
            continue;
        }
        if package.contains(&hint_lc) {
            contains.push(candidate.clone());
        }
    }

    ResolverDecision {
        exact,
        prefix,
        contains,
    }
}

#[derive(Debug, Default)]
pub struct NullSearch;

impl PackageSearch for NullSearch {
    fn search(&self, _hint: &str, _method: &InstallMethod) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
pub struct ShellPackageSearch;

impl PackageSearch for ShellPackageSearch {
    fn search(&self, hint: &str, method: &InstallMethod) -> Result<Vec<String>> {
        let (bin, args): (&str, Vec<String>) = match method {
            InstallMethod::Brew | InstallMethod::BrewCask => {
                ("brew", vec!["search".to_string(), hint.to_string()])
            }
            InstallMethod::Apt => ("apt-cache", vec!["search".to_string(), hint.to_string()]),
            InstallMethod::Dnf => ("dnf", vec!["search".to_string(), hint.to_string()]),
            InstallMethod::Pacman => ("pacman", vec!["-Ss".to_string(), hint.to_string()]),
            InstallMethod::Xbps => ("xbps-query", vec!["-Rs".to_string(), hint.to_string()]),
            InstallMethod::Snap | InstallMethod::SnapClassic => {
                ("snap", vec!["find".to_string(), hint.to_string()])
            }
            _ => return Ok(Vec::new()),
        };

        let output = Command::new(bin).args(args).output();
        let output = match output {
            Ok(output) if output.status.success() => output,
            _ => return Ok(Vec::new()),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed = stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        Ok(parsed)
    }
}
