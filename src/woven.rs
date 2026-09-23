// src/woven.rs
//! Presentation sections and relationships shared by output backends.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::identifier::{IdentifierIndex, IdentifierKind, IdentifierRole};
use crate::parser::{BlockKind, Modifier, Program, SourceOrigin};
use crate::resolver::{BlockLookup, ResolvedProgram};
use crate::util::block_reference;

// Presentation section locations
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SectionLocation {
    pub chapter: usize,
    pub chapter_number: String,
    pub section: usize,
}

impl SectionLocation {
    pub fn anchor(&self) -> String {
        format!("{}:{}", self.chapter_number, self.section)
    }

    pub fn label(&self, current_chapter: Option<usize>) -> String {
        if Some(self.chapter) == current_chapter {
            self.section.to_string()
        } else {
            format!("{}.{}", self.chapter_number, self.section)
        }
    }
}

// Presentation sections

#[derive(Debug)]
pub(crate) struct WeaveSection {
    pub source_section: usize,
    pub source_section_start: bool,
    pub prose: Option<usize>,
    pub code: Option<usize>,
    pub location: SectionLocation,
}

impl WeaveSection {
    fn new(
        chapter: usize,
        chapter_number: &str,
        source_section: usize,
        source_section_start: bool,
        number: usize,
    ) -> Self {
        Self {
            source_section,
            source_section_start,
            prose: None,
            code: None,
            location: SectionLocation {
                chapter,
                chapter_number: chapter_number.to_owned(),
                section: number,
            },
        }
    }
}

// Weave layout model

#[derive(Debug)]
pub(crate) struct WeaveLayout {
    pub chapters: Vec<Vec<WeaveSection>>,
    block_locations: Vec<Vec<Vec<Option<SectionLocation>>>>,
}

// Weave layout implementation
impl WeaveLayout {
    // Derive the weave layout

    pub fn new(program: &Program) -> Self {
        let mut chapters = Vec::with_capacity(program.chapters.len());
        let mut block_locations = Vec::with_capacity(program.chapters.len());

        for (chapter_index, chapter) in program.chapters.iter().enumerate() {
            let mut sections = Vec::new();
            let mut chapter_locations = Vec::with_capacity(chapter.sections.len());
            let chapter_number = chapter.number();

            for (source_section_index, source_section) in
                chapter.sections.iter().enumerate()
            {
                let mut locations = vec![None; source_section.blocks.len()];
                let mut current = WeaveSection::new(
                    chapter_index,
                    &chapter_number,
                    source_section_index,
                    true,
                    sections.len() + 1,
                );

                for (block_index, block) in source_section.blocks.iter().enumerate() {
                    match &block.kind {
                        BlockKind::Prose => {
                            if !block
                                .lines
                                .iter()
                                .any(|line| !line.text.trim().is_empty())
                            {
                                continue;
                            }
                            if current.prose.is_some() || current.code.is_some() {
                                sections.push(current);
                                current = WeaveSection::new(
                                    chapter_index,
                                    &chapter_number,
                                    source_section_index,
                                    false,
                                    sections.len() + 1,
                                );
                            }
                            current.prose = Some(block_index);
                            locations[block_index] = Some(current.location.clone());
                        }
                        BlockKind::Code(code) => {
                            if code.modifiers.contains(&Modifier::NoWeave) {
                                continue;
                            }
                            if current.code.is_some() {
                                sections.push(current);
                                current = WeaveSection::new(
                                    chapter_index,
                                    &chapter_number,
                                    source_section_index,
                                    false,
                                    sections.len() + 1,
                                );
                            }
                            current.code = Some(block_index);
                            locations[block_index] = Some(current.location.clone());
                        }
                    }
                }

                sections.push(current);
                chapter_locations.push(locations);
            }

            chapters.push(sections);
            block_locations.push(chapter_locations);
        }

        Self {
            chapters,
            block_locations,
        }
    }

    // Find a parsed block location

    pub fn block_location(
        &self,
        chapter: usize,
        source_section: usize,
        block: usize,
    ) -> Option<&SectionLocation> {
        self.block_locations[chapter][source_section][block].as_ref()
    }
}

// Woven relationship index model
#[derive(Debug, Default)]
pub(crate) struct BlockLocations {
    pub definition: Option<SectionLocation>,
    pub definition_hidden: bool,
    pub additions: Vec<SectionLocation>,
    pub redefinitions: Vec<SectionLocation>,
    pub uses: Vec<SectionLocation>,
}

pub(crate) struct WeaveIndex {
    pub blocks: HashMap<usize, BlockLocations>,
    pub roots: HashSet<usize>,
    chapter_paths: Option<Vec<PathBuf>>,
}

// Resolve book chapter links
impl WeaveIndex {
    pub fn chapter_link(
        &self,
        target: &str,
        requesting_chapter: Option<usize>,
    ) -> Result<Option<usize>, ()> {
        let Some(paths) = &self.chapter_paths else {
            return Ok(None);
        };
        if !target.ends_with(".lit")
            || target.starts_with('/')
            || target.contains([':', '?', '#', '\\'])
        {
            return Ok(None);
        }
        let mut path = requesting_chapter
            .and_then(|chapter| paths[chapter].parent())
            .unwrap_or_else(|| std::path::Path::new(""))
            .to_path_buf();
        for component in target.split('/') {
            match component {
                "" => return Err(()),
                "." => {}
                ".." => {
                    if !path.pop() {
                        return Err(());
                    }
                }
                name => path.push(name),
            }
        }
        paths
            .iter()
            .position(|candidate| *candidate == path)
            .map(Some)
            .ok_or(())
    }
}

// Located identifier index
#[derive(Default)]
pub(crate) struct LocatedIdentifierIndex {
    pub entries: Vec<LocatedIdentifierEntry>,
}

pub(crate) struct LocatedIdentifierEntry {
    pub name: String,
    pub locations: Vec<LocatedIdentifierLocation>,
}

pub(crate) struct LocatedIdentifierLocation {
    pub location: SectionLocation,
    pub role: IdentifierRole,
}

pub(crate) fn locate_identifiers(
    source: &IdentifierIndex,
    layout: &WeaveLayout,
) -> LocatedIdentifierIndex {
    let mut entries = Vec::new();
    for entry in source.entries() {
        let mut locations: Vec<LocatedIdentifierLocation> = Vec::new();
        for occurrence in &entry.occurrences {
            let Some(location) = layout.block_location(
                occurrence.site.chapter,
                occurrence.site.section,
                occurrence.site.block,
            ) else {
                continue;
            };
            if let Some(existing) = locations
                .iter_mut()
                .find(|existing| existing.location == *location)
            {
                if occurrence.role == IdentifierRole::Definition {
                    existing.role = IdentifierRole::Definition;
                }
            } else {
                locations.push(LocatedIdentifierLocation {
                    location: location.clone(),
                    role: occurrence.role,
                });
            }
        }
        if !locations.is_empty() {
            entries.push(LocatedIdentifierEntry {
                name: entry.name.clone(),
                locations,
            });
        }
    }
    LocatedIdentifierIndex { entries }
}

// Located mini-index meanings
#[derive(Default)]
pub(crate) struct LocatedMiniIndex {
    pub meanings: Vec<LocatedMiniMeaning>,
    pub occurrences: HashMap<SourceOrigin, Vec<LocatedMiniOccurrence>>,
}

pub(crate) struct LocatedMiniMeaning {
    pub marker: usize,
    pub name: String,
    pub kind: IdentifierKind,
    pub definition: SectionLocation,
}

#[derive(Clone, Copy)]
pub(crate) struct LocatedMiniOccurrence {
    pub marker: usize,
    pub role: IdentifierRole,
    pub column: usize,
}

pub(crate) fn locate_mini_identifiers(
    source: &IdentifierIndex,
    layout: &WeaveLayout,
) -> LocatedMiniIndex {
    let mut selected = source
        .meanings()
        .iter()
        .filter_map(|meaning| {
            layout
                .block_location(
                    meaning.definition.chapter,
                    meaning.definition.section,
                    meaning.definition.block,
                )
                .cloned()
                .map(|definition| (meaning, definition))
        })
        .collect::<Vec<_>>();
    selected.sort_by(|(left, left_location), (right, right_location)| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left_location.chapter.cmp(&right_location.chapter))
            .then_with(|| left_location.section.cmp(&right_location.section))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut source_to_marker = vec![None; source.meanings().len()];
    let meanings = selected
        .into_iter()
        .enumerate()
        .map(|(position, (meaning, definition))| {
            let marker = position + 1;
            source_to_marker[meaning.id] = Some(marker);
            LocatedMiniMeaning {
                marker,
                name: meaning.name.clone(),
                kind: meaning.kind,
                definition,
            }
        })
        .collect::<Vec<_>>();

    let mut occurrences: HashMap<SourceOrigin, Vec<LocatedMiniOccurrence>> =
        HashMap::new();
    for occurrence in source.meaning_occurrences() {
        let Some(marker) = source_to_marker[occurrence.meaning] else {
            continue;
        };
        if layout
            .block_location(
                occurrence.site.chapter,
                occurrence.site.section,
                occurrence.site.block,
            )
            .is_none()
        {
            continue;
        }
        occurrences
            .entry(occurrence.site.origin.clone())
            .or_default()
            .push(LocatedMiniOccurrence {
                marker,
                role: occurrence.role,
                column: occurrence.column,
            });
    }
    for located in occurrences.values_mut() {
        located.sort_by_key(|occurrence| (occurrence.column, occurrence.marker));
    }

    LocatedMiniIndex {
        meanings,
        occurrences,
    }
}

// Collect weave locations
pub(crate) fn collect_locations(
    program: &Program,
    resolved: &ResolvedProgram,
    layout: &WeaveLayout,
) -> WeaveIndex {
    let mut blocks = (0..resolved.blocks.len())
        .map(|block| (block, BlockLocations::default()))
        .collect::<HashMap<_, _>>();
    let roots = resolved.roots.iter().map(|root| root.block).collect();

    for (chapter_index, chapter) in program.chapters.iter().enumerate() {
        for (section_index, section) in chapter.sections.iter().enumerate() {
            for (block_index, block) in section.blocks.iter().enumerate() {
                // Index a code occurrence
                let Some(code) = block.code() else {
                    continue;
                };
                let BlockLookup::Found(identity) =
                    resolved.lookup(chapter_index, &code.name)
                else {
                    continue;
                };
                let locations = blocks.entry(identity).or_default();
                let hidden = code.modifiers.contains(&Modifier::NoWeave);
                if code.modifiers.contains(&Modifier::Additive) {
                    if let Some(location) =
                        layout.block_location(chapter_index, section_index, block_index)
                    {
                        push_unique(&mut locations.additions, location.clone());
                    }
                } else if code.modifiers.contains(&Modifier::Redefinition) {
                    if let Some(location) =
                        layout.block_location(chapter_index, section_index, block_index)
                    {
                        push_unique(&mut locations.redefinitions, location.clone());
                    }
                } else if hidden {
                    locations.definition_hidden = true;
                } else if let Some(location) =
                    layout.block_location(chapter_index, section_index, block_index)
                {
                    locations.definition = Some(location.clone());
                }

                // Index whole-line uses
                let Some(location) =
                    layout.block_location(chapter_index, section_index, block_index)
                else {
                    continue;
                };
                for line in &block.lines {
                    if let Some(name) = block_reference(&line.text)
                        && let BlockLookup::Found(target) =
                            resolved.lookup(chapter_index, name)
                    {
                        push_unique(
                            &mut blocks.entry(target).or_default().uses,
                            location.clone(),
                        );
                    }
                }
            }
        }
    }

    let chapter_paths = program.book().map(|_| {
        program
            .chapters
            .iter()
            .map(|chapter| {
                chapter
                    .book
                    .as_ref()
                    .expect("book chapter")
                    .source_path
                    .clone()
            })
            .collect()
    });
    WeaveIndex {
        blocks,
        roots,
        chapter_paths,
    }
}

fn push_unique(locations: &mut Vec<SectionLocation>, location: SectionLocation) {
    if !locations.contains(&location) {
        locations.push(location);
    }
}
