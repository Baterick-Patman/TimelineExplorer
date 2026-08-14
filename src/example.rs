//! A small worked example: the Roman Republic against the Hellenistic
//! successor kingdoms, with two biographies.
//!
//! This exists to demonstrate the features on real data — in particular the
//! convergence case from the brief, where Macedon and the Seleucid and
//! Ptolemaic kingdoms are absorbed into Rome at their historical dates. It is
//! offered, never forced; the user can start empty or delete it wholesale.

use crate::model::*;

/// Look a category up by name in the starter set.
fn cat(doc: &Document, name: &str) -> Vec<Id> {
    doc.categories
        .iter()
        .find(|c| c.name == name)
        .map(|c| vec![c.id])
        .unwrap_or_default()
}

fn cats(doc: &Document, names: &[&str]) -> Vec<Id> {
    names.iter().flat_map(|n| cat(doc, n)).collect()
}

pub fn build() -> Document {
    let mut doc = Document::with_starter_categories();

    // --- Groups ------------------------------------------------------------
    // Super-categories: collapse one to compare whole civilisations at a
    // glance, expand it to work with the individual timelines inside.
    let greek = doc.new_id();
    doc.groups.push(Group {
        id: greek,
        name: "Greek antiquity".into(),
        color: [96, 170, 200],
        parent: None,
        order: 0,
        collapsed: false,
        visible: true,
        notes: "Classical poleis and the Hellenistic successor kingdoms.".into(),
    });
    let hellenistic = doc.new_id();
    doc.groups.push(Group {
        id: hellenistic,
        name: "Hellenistic world".into(),
        color: [150, 160, 210],
        parent: Some(greek),
        order: 1,
        collapsed: false,
        visible: true,
        notes: String::new(),
    });
    let classical = doc.new_id();
    doc.groups.push(Group {
        id: classical,
        name: "Classical poleis".into(),
        color: [110, 190, 175],
        parent: Some(greek),
        order: 0,
        collapsed: false,
        visible: true,
        notes: String::new(),
    });
    let roman = doc.new_id();
    doc.groups.push(Group {
        id: roman,
        name: "Roman world".into(),
        color: [214, 96, 77],
        parent: None,
        order: 1,
        collapsed: false,
        visible: true,
        notes: String::new(),
    });

    // --- Timelines ---------------------------------------------------------
    let rome = doc.new_id();
    doc.timelines.push(Timeline {
        id: rome,
        name: "Roman Republic".into(),
        color: [214, 96, 77],
        visible: true,
        group: Some(roman),
        order: 0,
        span: Some(Span::range(HDate::year(-509), HDate::year(-27))),
        origin: None,
        merge: None,
        notes: "From the expulsion of the kings to the Augustan settlement.".into(),
    });

    let alexander = doc.new_id();
    doc.timelines.push(Timeline {
        id: alexander,
        name: "Empire of Alexander".into(),
        color: [216, 160, 70],
        visible: true,
        group: Some(hellenistic),
        order: 1,
        // Runs to Ipsus rather than to Alexander's death, so that the three
        // successor kingdoms visibly split off from a band that still exists
        // at the dates they were founded.
        span: Some(Span::range(HDate::year(-336), HDate::year(-301))),
        origin: None,
        merge: None,
        notes: "Fragmented among the Diadochi after Alexander's death.".into(),
    });

    // Three successor kingdoms: they split from Alexander's empire and are
    // each absorbed by Rome. This is the convergence/divergence showcase.
    let macedon = doc.new_id();
    doc.timelines.push(Timeline {
        id: macedon,
        name: "Antigonid Macedon".into(),
        color: [83, 141, 213],
        visible: true,
        group: Some(hellenistic),
        order: 2,
        span: Some(Span::range(HDate::year(-306), HDate::year(-168))),
        origin: Some(Junction {
            other: alexander,
            date: HDate::year(-306),
            label: "Wars of the Diadochi".into(),
        }),
        merge: Some(Junction {
            other: rome,
            date: HDate::year(-168),
            label: "Pydna".into(),
        }),
        notes: String::new(),
    });

    let seleucid = doc.new_id();
    doc.timelines.push(Timeline {
        id: seleucid,
        name: "Seleucid Empire".into(),
        color: [95, 178, 130],
        visible: true,
        group: Some(hellenistic),
        order: 3,
        span: Some(Span::range(HDate::year(-312), HDate::year(-63))),
        origin: Some(Junction {
            other: alexander,
            date: HDate::year(-312),
            label: String::new(),
        }),
        merge: Some(Junction {
            other: rome,
            date: HDate::year(-63),
            label: "Annexed by Pompey".into(),
        }),
        notes: String::new(),
    });

    let ptolemaic = doc.new_id();
    doc.timelines.push(Timeline {
        id: ptolemaic,
        name: "Ptolemaic Egypt".into(),
        color: [163, 120, 206],
        visible: true,
        group: Some(hellenistic),
        order: 4,
        span: Some(Span::range(HDate::year(-305), HDate::year(-30))),
        origin: Some(Junction {
            other: alexander,
            date: HDate::year(-305),
            label: String::new(),
        }),
        merge: Some(Junction {
            other: rome,
            date: HDate::year(-30),
            label: "Death of Cleopatra VII".into(),
        }),
        notes: String::new(),
    });

    // --- Events ------------------------------------------------------------
    let add = |doc: &mut Document,
                   owner: OwnerRef,
                   title: &str,
                   span: Span,
                   importance: u8,
                   categories: Vec<Id>| {
        let id = doc.new_id();
        doc.events.push(Event {
            id,
            owner,
            title: title.into(),
            description: String::new(),
            span,
            importance,
            categories,
        });
    };

    let military = cat(&doc, "Military");
    let politics = cat(&doc, "Politics");
    let mil_pol = cats(&doc, &["Military", "Politics"]);
    let literature = cat(&doc, "Literature");
    let philosophy = cat(&doc, "Philosophy");
    let science = cat(&doc, "Science");
    let personal = cat(&doc, "Personal");
    let law = cat(&doc, "Law");
    let art = cat(&doc, "Art");
    let mil_rel = cats(&doc, &["Military", "Religion"]);
    let sci_lit = cats(&doc, &["Science", "Literature"]);
    let lit_law = cats(&doc, &["Literature", "Law"]);
    let lit_phil = cats(&doc, &["Literature", "Philosophy"]);

    let r = OwnerRef::Timeline(rome);
    add(&mut doc, r, "Founding of the Republic", Span::point(HDate::year(-509)), 5, politics.clone());
    add(&mut doc, r, "Twelve Tables", Span::point(HDate::year(-451)), 4, law.clone());
    add(&mut doc, r, "First Punic War", Span::range(HDate::year(-264), HDate::year(-241)), 5, military.clone());
    add(&mut doc, r, "Second Punic War", Span::range(HDate::year(-218), HDate::year(-201)), 5, military.clone());
    add(&mut doc, r, "Battle of Cannae", Span::point(HDate::year(-216)), 4, military.clone());
    add(&mut doc, r, "Destruction of Carthage", Span::point(HDate::year(-146)), 5, military.clone());
    add(&mut doc, r, "Gracchan reforms", Span::range(HDate::year(-133), HDate::year(-121)), 3, politics.clone());
    add(&mut doc, r, "Social War", Span::range(HDate::year(-91), HDate::year(-88)), 3, military.clone());
    add(&mut doc, r, "Dictatorship of Sulla", Span::range(HDate::year(-82), HDate::year(-79)), 4, mil_pol.clone());
    add(&mut doc, r, "Catilinarian conspiracy", Span::point(HDate::year(-63)), 2, politics.clone());
    add(&mut doc, r, "First Triumvirate", Span::point(HDate::year(-60)), 3, politics.clone());
    add(&mut doc, r, "Caesar's civil war", Span::range(HDate::year(-49), HDate::year(-45)), 4, mil_pol.clone());
    add(&mut doc, r, "Assassination of Caesar", Span::point(HDate { month: Some(3), day: Some(15), ..HDate::year(-44) }), 5, mil_pol.clone());
    add(&mut doc, r, "Battle of Actium", Span::point(HDate::year(-31)), 5, military.clone());

    let a = OwnerRef::Timeline(alexander);
    add(&mut doc, a, "Alexander succeeds Philip II", Span::point(HDate::year(-336)), 5, politics.clone());
    add(&mut doc, a, "Battle of Gaugamela", Span::point(HDate::year(-331)), 4, military.clone());
    add(&mut doc, a, "Death of Alexander", Span::point(HDate::year(-323)), 5, politics.clone());
    add(&mut doc, a, "Wars of the Diadochi", Span::range(HDate::year(-322), HDate::year(-301)), 4, mil_pol.clone());

    let m = OwnerRef::Timeline(macedon);
    add(&mut doc, m, "Second Macedonian War", Span::range(HDate::year(-200), HDate::year(-197)), 3, military.clone());
    add(&mut doc, m, "Battle of Pydna", Span::point(HDate::year(-168)), 5, military.clone());

    let s = OwnerRef::Timeline(seleucid);
    add(&mut doc, s, "Battle of Magnesia", Span::point(HDate::year(-190)), 4, military.clone());
    add(&mut doc, s, "Maccabean revolt", Span::range(HDate::year(-167), HDate::year(-160)), 3, mil_rel.clone());
    add(&mut doc, s, "Pompey annexes Syria", Span::point(HDate::year(-63)), 4, mil_pol.clone());

    let p = OwnerRef::Timeline(ptolemaic);
    add(&mut doc, p, "Library of Alexandria founded", Span::circa_point(-295), 4, sci_lit.clone());
    add(&mut doc, p, "Eratosthenes measures the Earth", Span::circa_point(-240), 3, science.clone());
    add(&mut doc, p, "Death of Cleopatra VII", Span::point(HDate::year(-30)), 5, politics.clone());

    // --- Biographies -------------------------------------------------------
    let cicero = doc.new_id();
    doc.biographies.push(Biography {
        id: cicero,
        name: "Cicero".into(),
        timeline: Some(rome),
        birth: HDate::year(-106),
        death: Some(HDate::year(-43)),
        color: Some([232, 178, 96]),
        categories: cats(&doc, &["Literature", "Philosophy", "Politics"]),
        importance: 4,
        display: BioDisplay::Inline,
        notes: "Orator, consul, and prolific correspondent.".into(),
    });

    let caesar = doc.new_id();
    doc.biographies.push(Biography {
        id: caesar,
        name: "Julius Caesar".into(),
        timeline: Some(rome),
        birth: HDate::year(-100),
        death: Some(HDate {
            month: Some(3),
            day: Some(15),
            ..HDate::year(-44)
        }),
        color: Some([236, 130, 110]),
        categories: cats(&doc, &["Politics", "Military", "Literature"]),
        importance: 5,
        display: BioDisplay::Lane,
        notes: String::new(),
    });

    let c = OwnerRef::Biography(cicero);
    add(&mut doc, c, "Born at Arpinum", Span::point(HDate::year(-106)), 3, personal.clone());
    add(&mut doc, c, "Pro Roscio Amerino", Span::point(HDate::year(-80)), 2, lit_law.clone());
    add(&mut doc, c, "Consulship", Span::point(HDate::year(-63)), 4, politics.clone());
    add(&mut doc, c, "Exile", Span::range(HDate::year(-58), HDate::year(-57)), 2, personal.clone());
    add(&mut doc, c, "De re publica", Span::range(HDate::year(-54), HDate::year(-51)), 3, lit_phil.clone());
    add(&mut doc, c, "Philippics", Span::range(HDate::year(-44), HDate::year(-43)), 3, literature.clone());
    add(&mut doc, c, "Proscribed and killed", Span::point(HDate::year(-43)), 4, politics.clone());

    let cs = OwnerRef::Biography(caesar);
    add(&mut doc, cs, "Born", Span::point(HDate::year(-100)), 3, personal.clone());
    add(&mut doc, cs, "Consulship", Span::point(HDate::year(-59)), 3, politics.clone());
    add(&mut doc, cs, "Gallic Wars", Span::range(HDate::year(-58), HDate::year(-50)), 5, military.clone());
    add(&mut doc, cs, "Commentarii de Bello Gallico", Span::circa_point(-51), 3, literature.clone());
    add(&mut doc, cs, "Crosses the Rubicon", Span::point(HDate::year(-49)), 5, mil_pol.clone());
    add(&mut doc, cs, "Dictator perpetuo", Span::point(HDate::year(-44)), 4, politics.clone());
    add(&mut doc, cs, "Assassinated", Span::point(HDate { month: Some(3), day: Some(15), ..HDate::year(-44) }), 5, politics.clone());

    // --- Classical Athens and Sparta ---------------------------------------
    // This part exists to show the fine-grained case: individual tragedies
    // lined up, year by year, against the events of the Peloponnesian War.
    let athens = doc.new_id();
    doc.timelines.push(Timeline {
        id: athens,
        name: "Athens".into(),
        color: [86, 178, 190],
        visible: true,
        group: Some(classical),
        order: 0,
        span: Some(Span::range(HDate::year(-508), HDate::year(-322))),
        origin: None,
        merge: None,
        notes: "Democracy, empire and the tragic stage.".into(),
    });
    let sparta = doc.new_id();
    doc.timelines.push(Timeline {
        id: sparta,
        name: "Sparta".into(),
        color: [199, 150, 96],
        visible: true,
        group: Some(classical),
        order: 1,
        span: Some(Span::range(HDate::year(-550), HDate::year(-371))),
        origin: None,
        merge: None,
        notes: String::new(),
    });

    let at = OwnerRef::Timeline(athens);
    add(&mut doc, at, "Cleisthenic reforms", Span::point(HDate::year(-508)), 4, politics.clone());
    add(&mut doc, at, "Battle of Marathon", Span::point(HDate::year(-490)), 5, military.clone());
    add(&mut doc, at, "Battle of Salamis", Span::point(HDate::year(-480)), 5, military.clone());
    add(&mut doc, at, "Delian League founded", Span::point(HDate::year(-478)), 4, politics.clone());
    add(&mut doc, at, "Parthenon begun", Span::point(HDate::year(-447)), 3, art.clone());
    add(&mut doc, at, "Peloponnesian War", Span::range(HDate::year(-431), HDate::year(-404)), 5, military.clone());
    add(&mut doc, at, "Plague of Athens", Span::range(HDate::year(-430), HDate::year(-426)), 4, personal.clone());
    add(&mut doc, at, "Death of Pericles", Span::point(HDate::year(-429)), 4, politics.clone());
    add(&mut doc, at, "Peace of Nicias", Span::point(HDate::year(-421)), 3, politics.clone());
    add(&mut doc, at, "Melian dialogue", Span::point(HDate::year(-416)), 2, mil_pol.clone());
    add(&mut doc, at, "Sicilian Expedition", Span::range(HDate::year(-415), HDate::year(-413)), 4, military.clone());
    add(&mut doc, at, "Surrender of Athens", Span::point(HDate::year(-404)), 5, military.clone());
    add(&mut doc, at, "Trial of Socrates", Span::point(HDate::year(-399)), 4, philosophy.clone());

    let sp = OwnerRef::Timeline(sparta);
    add(&mut doc, sp, "Battle of Thermopylae", Span::point(HDate::year(-480)), 5, military.clone());
    add(&mut doc, sp, "Sparta enters the war", Span::point(HDate::year(-431)), 3, military.clone());
    add(&mut doc, sp, "Battle of Aegospotami", Span::point(HDate::year(-405)), 4, military.clone());
    add(&mut doc, sp, "Battle of Leuctra", Span::point(HDate::year(-371)), 4, military.clone());

    // Two tragedians whose surviving plays can be dated to the year, so their
    // works sit directly above the war events they were written during.
    let sophocles = doc.new_id();
    doc.biographies.push(Biography {
        id: sophocles,
        name: "Sophocles".into(),
        timeline: Some(athens),
        birth: HDate::circa(-497),
        death: Some(HDate::year(-406)),
        color: Some([120, 205, 215]),
        categories: lit_phil.clone(),
        importance: 4,
        display: BioDisplay::Inline,
        notes: "Tragedian; seven plays survive.".into(),
    });
    let euripides = doc.new_id();
    doc.biographies.push(Biography {
        id: euripides,
        name: "Euripides".into(),
        timeline: Some(athens),
        birth: HDate::circa(-480),
        death: Some(HDate::year(-406)),
        color: Some([170, 210, 140]),
        categories: literature.clone(),
        importance: 4,
        display: BioDisplay::Inline,
        notes: "Tragedian; his late plays track the war closely.".into(),
    });

    let so = OwnerRef::Biography(sophocles);
    add(&mut doc, so, "Ajax", Span::circa_point(-445), 2, literature.clone());
    add(&mut doc, so, "Antigone", Span::point(HDate::year(-441)), 3, literature.clone());
    add(&mut doc, so, "Oedipus Rex", Span::circa_point(-429), 3, literature.clone());
    add(&mut doc, so, "Electra", Span::circa_point(-413), 2, literature.clone());
    add(&mut doc, so, "Philoctetes", Span::point(HDate::year(-409)), 2, literature.clone());
    // Staged five years after his death: the lane has to carry it past the
    // end of the lifeline.
    add(&mut doc, so, "Oedipus at Colonus (posthumous)", Span::point(HDate::year(-401)), 2, literature.clone());

    let eu = OwnerRef::Biography(euripides);
    add(&mut doc, eu, "Medea", Span::point(HDate::year(-431)), 3, literature.clone());
    add(&mut doc, eu, "Hippolytus", Span::point(HDate::year(-428)), 2, literature.clone());
    add(&mut doc, eu, "Hecuba", Span::circa_point(-424), 2, literature.clone());
    add(&mut doc, eu, "Trojan Women", Span::point(HDate::year(-415)), 3, literature.clone());
    add(&mut doc, eu, "Helen", Span::point(HDate::year(-412)), 2, literature.clone());
    add(&mut doc, eu, "Bacchae", Span::circa_point(-405), 3, literature.clone());

    // Frame the whole thing on first view.
    doc.view.left_year = -580.0;
    doc.view.pixels_per_year = 2.2;
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout;

    #[test]
    fn example_is_internally_consistent() {
        let doc = build();
        // Every event points at something that exists.
        for e in &doc.events {
            match e.owner {
                OwnerRef::Timeline(id) => assert!(
                    doc.timeline(id).is_some(),
                    "event {:?} has a dangling timeline",
                    e.title
                ),
                OwnerRef::Biography(id) => assert!(
                    doc.biography(id).is_some(),
                    "event {:?} has a dangling biography",
                    e.title
                ),
            }
            for c in &e.categories {
                assert!(doc.category(*c).is_some(), "unknown category on {}", e.title);
            }
        }
        // Every junction points at an existing timeline.
        for t in &doc.timelines {
            for j in [t.origin.as_ref(), t.merge.as_ref()].into_iter().flatten() {
                assert!(doc.timeline(j.other).is_some(), "dangling junction on {}", t.name);
                assert_ne!(j.other, t.id, "{} joins itself", t.name);
            }
        }
        for b in &doc.biographies {
            if let Some(t) = b.timeline {
                assert!(doc.timeline(t).is_some());
            }
        }
    }

    #[test]
    fn example_groups_form_a_valid_nested_tree() {
        let doc = build();
        assert!(!doc.groups.is_empty());
        for g in &doc.groups {
            if let Some(p) = g.parent {
                assert!(doc.group(p).is_some(), "{} has a dangling parent", g.name);
                assert!(!doc.would_cycle(g.id, g.parent), "{} is cyclic", g.name);
            }
        }
        for t in &doc.timelines {
            if let Some(g) = t.group {
                assert!(doc.group(g).is_some(), "{} has a dangling group", t.name);
            }
        }
        // At least one group nests inside another, so the feature is on display.
        assert!(doc.groups.iter().any(|g| g.parent.is_some()));
    }

    #[test]
    fn collapsing_a_group_still_accounts_for_every_member_timeline() {
        let doc = build();
        let greek = doc
            .groups
            .iter()
            .find(|g| g.name == "Greek antiquity")
            .expect("example should have a Greek antiquity group");
        let members = doc.group_timelines(greek.id);
        // Sub-group members must be reached through the subtree walk.
        assert!(members.len() >= 5, "got {members:?}");
        for name in ["Athens", "Sparta", "Ptolemaic Egypt"] {
            let id = doc.timelines.iter().find(|t| t.name == name).unwrap().id;
            assert!(members.contains(&id), "{name} missing from the group subtree");
        }
    }

    #[test]
    fn the_tragedians_line_up_with_the_peloponnesian_war() {
        // The point of the example: individual plays sit at their own year,
        // against the war events on the parent culture's timeline.
        let doc = build();
        let war = doc
            .events
            .iter()
            .find(|e| e.title == "Peloponnesian War")
            .expect("war event");
        let (w0, w1) = (war.span.t0(), war.span.t1());

        for author in ["Sophocles", "Euripides"] {
            let bio = doc.biographies.iter().find(|b| b.name == author).unwrap();
            let plays: Vec<&Event> = doc.events_of(OwnerRef::Biography(bio.id)).collect();
            assert!(plays.len() >= 5, "{author} should have several dated works");
            assert!(
                plays.iter().any(|p| p.span.t0() >= w0 && p.span.t0() <= w1),
                "{author} should have work dated inside the war"
            );
            // Works must not predate their author. They may postdate their
            // death — Oedipus at Colonus was staged in 401 BC, five years after
            // Sophocles died — so a biography lane has to tolerate posthumous
            // entries rather than assume everything fits inside the lifespan.
            let life = bio.span();
            for p in &plays {
                assert!(
                    p.span.t0() >= life.t0(),
                    "{} is dated before {} was born",
                    p.title,
                    author
                );
            }
        }
    }

    #[test]
    fn play_level_detail_appears_only_when_zoomed_in() {
        let doc = build();
        let filters = Filters::default();
        let antigone = doc.events.iter().find(|e| e.title == "Antigone").unwrap();
        assert!(
            !layout::event_visible(antigone, &filters, 0.2),
            "individual plays should not clutter the fully zoomed-out view"
        );
        assert!(
            layout::event_visible(antigone, &filters, 60.0),
            "they must appear once zoomed in to single years"
        );
    }

    #[test]
    fn example_ids_are_unique() {
        let doc = build();
        let mut ids: Vec<u32> = doc
            .timelines
            .iter()
            .map(|t| t.id.0)
            .chain(doc.groups.iter().map(|g| g.id.0))
            .chain(doc.biographies.iter().map(|b| b.id.0))
            .chain(doc.events.iter().map(|e| e.id.0))
            .chain(doc.categories.iter().map(|c| c.id.0))
            .collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "ids must be unique across the document");
    }

    #[test]
    fn example_demonstrates_convergence_and_divergence() {
        let doc = build();
        let merging = doc.timelines.iter().filter(|t| t.merge.is_some()).count();
        let splitting = doc.timelines.iter().filter(|t| t.origin.is_some()).count();
        assert!(merging >= 3, "the merge case should be on display");
        assert!(splitting >= 3, "the split case should be on display");
    }

    #[test]
    fn example_covers_both_biography_display_modes() {
        let doc = build();
        assert!(doc.biographies.iter().any(|b| b.display == BioDisplay::Inline));
        assert!(doc.biographies.iter().any(|b| b.display == BioDisplay::Lane));
    }

    #[test]
    fn example_spans_a_sensible_range_and_frames_itself() {
        let doc = build();
        let (lo, hi) = doc.extent().expect("example must have an extent");
        assert!(lo < -500.0 && hi > -40.0, "got {lo}..{hi}");
        assert!(doc.view.left_year <= lo, "initial view should include the start");
    }

    #[test]
    fn every_timeline_yields_a_drawable_band() {
        let doc = build();
        for t in &doc.timelines {
            let range = layout::timeline_band_range(&doc, t);
            assert!(range.is_some(), "{} has no band", t.name);
            let (lo, hi) = range.unwrap();
            assert!(hi > lo, "{} has an inverted band", t.name);
        }
    }

    #[test]
    fn some_events_survive_at_the_widest_zoom() {
        let doc = build();
        let filters = Filters::default();
        let visible = doc
            .events
            .iter()
            .filter(|e| layout::event_visible(e, &filters, 0.1))
            .count();
        assert!(visible > 0, "zoomed right out the chart must not be blank");
        // ...but not everything, or the zoom filter is doing nothing.
        assert!(visible < doc.events.len());
    }
}
