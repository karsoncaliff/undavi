//! Keyboard focus: Tab order, arrow-key stepping and Enter/Space activation.
//!
//! iced 0.14 only makes text inputs focusable, so Undavi keeps its own focus model.
//! While the view is built, every focusable control registers itself here in
//! reading order, together with what Enter/Space and the arrow keys should do.
//! Key handling then looks up the focused control in the registry from the
//! last view, so the view stays the single src of truth for focus order.

use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FocusId {
    Named(&'static str),
    /// One of several similar controls, e.g. a preset row or picker entry.
    Indexed(&'static str, usize),
}

/// Where a control lives. Tab stays inside the picker while it's open, and panel
/// controls scroll into view when focused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Main,
    Panel,
    Picker,
}

/// An adjustment from the arrow, Page and Home/End keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Less,
    More,
    MuchLess,
    MuchMore,
    Min,
    Max,
}

impl Step {
    /// Slider value after this step (`big` is the PageUp/PageDown amount).
    pub fn apply(self, value: f32, (min, max, step): (f32, f32, f32)) -> f32 {
        let big = step.max(((max - min) / 10.0 / step).round() * step);
        let v = match self {
            Step::Less => value - step,
            Step::More => value + step,
            Step::MuchLess => value - big,
            Step::MuchMore => value + big,
            Step::Min => min,
            Step::Max => max,
        };
        (min + ((v - min) / step).round() * step).clamp(min, max)
    }

    /// Index after this step in a list of `len` choices (no wrapping).
    pub fn apply_index(self, index: usize, len: usize) -> usize {
        let last = len.saturating_sub(1);
        match self {
            Step::Less | Step::MuchLess => index.saturating_sub(1),
            Step::More | Step::MuchMore => (index + 1).min(last),
            Step::Min => 0,
            Step::Max => last,
        }
    }
}

pub type Stepper<M> = Rc<dyn Fn(Step) -> Option<M>>;

pub struct Entry<M> {
    pub id: FocusId,
    pub scope: Scope,
    /// Sent on Enter or Space.
    pub activate: Option<M>,
    /// Handles arrows, PageUp/PageDown and Home/End.
    pub step: Option<Stepper<M>>,
    /// Set for text inputs, which take real iced focus while focused.
    pub text_input: Option<&'static str>,
}

pub struct Registry<M> {
    entries: RefCell<Vec<Entry<M>>>,
}

impl<M> Default for Registry<M> {
    fn default() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
        }
    }
}

impl<M: Clone> Registry<M> {
    pub fn clear(&self) {
        self.entries.borrow_mut().clear();
    }

    pub fn push(&self, entry: Entry<M>) {
        self.entries.borrow_mut().push(entry);
    }

    pub fn contains(&self, id: &FocusId) -> bool {
        self.entries.borrow().iter().any(|e| &e.id == id)
    }

    pub fn activate(&self, id: &FocusId) -> Option<M> {
        self.entries
            .borrow()
            .iter()
            .find(|e| &e.id == id)?
            .activate
            .clone()
    }

    pub fn step(&self, id: &FocusId, step: Step) -> Option<M> {
        let stepper = self
            .entries
            .borrow()
            .iter()
            .find(|e| &e.id == id)?
            .step
            .clone()?;
        stepper(step)
    }

    pub fn text_input(&self, id: &FocusId) -> Option<&'static str> {
        self.entries
            .borrow()
            .iter()
            .find(|e| &e.id == id)?
            .text_input
    }

    /// The next (or previous) focus stop, wrapping around. With `only`, Tab is
    /// confined to that scope.
    pub fn cycle(
        &self,
        current: Option<&FocusId>,
        back: bool,
        only: Option<Scope>,
    ) -> Option<FocusId> {
        let entries = self.entries.borrow();
        let stops: Vec<&Entry<M>> = entries
            .iter()
            .filter(|e| only.is_none_or(|s| e.scope == s))
            .collect();
        if stops.is_empty() {
            return None;
        }
        let n = stops.len();
        let next = match current.and_then(|c| stops.iter().position(|e| &e.id == c)) {
            Some(i) if back => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None if back => n - 1,
            None => 0,
        };
        Some(stops[next].id.clone())
    }

    /// Where `id` sits among the controls of its scope: (index, count, scope).
    pub fn position(&self, id: &FocusId) -> Option<(usize, usize, Scope)> {
        let entries = self.entries.borrow();
        let scope = entries.iter().find(|e| &e.id == id)?.scope;
        let same: Vec<&Entry<M>> = entries.iter().filter(|e| e.scope == scope).collect();
        let index = same.iter().position(|e| &e.id == id)?;
        Some((index, same.len(), scope))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &'static str, scope: Scope) -> Entry<u8> {
        Entry {
            id: FocusId::Named(name),
            scope,
            activate: Some(1),
            step: None,
            text_input: None,
        }
    }

    #[test]
    fn tab_order_wraps_and_respects_scope() {
        let r = Registry::default();
        r.push(entry("a", Scope::Main));
        r.push(entry("b", Scope::Picker));
        r.push(entry("c", Scope::Main));
        let a = FocusId::Named("a");
        let c = FocusId::Named("c");
        assert_eq!(r.cycle(None, false, None), Some(a.clone()));
        assert_eq!(r.cycle(Some(&c), false, None), Some(a.clone()));
        assert_eq!(r.cycle(Some(&a), true, None), Some(c.clone()));
        assert_eq!(
            r.cycle(Some(&a), false, Some(Scope::Picker)),
            Some(FocusId::Named("b"))
        );
        assert_eq!(r.position(&c), Some((1, 2, Scope::Main)));
    }

    #[test]
    fn slider_steps() {
        let range = (0.0, 100.0, 1.0);
        assert_eq!(Step::More.apply(72.0, range), 73.0);
        assert_eq!(Step::MuchLess.apply(72.0, range), 62.0);
        assert_eq!(Step::Max.apply(72.0, range), 100.0);
        assert_eq!(Step::Less.apply(0.0, range), 0.0);
        // Coarse sliders keep their step size.
        assert_eq!(Step::More.apply(180.0, (50.0, 800.0, 10.0)), 190.0);
        assert_eq!(Step::MuchMore.apply(180.0, (50.0, 800.0, 10.0)), 260.0);
        assert_eq!(Step::Less.apply_index(0, 3), 0);
        assert_eq!(Step::Max.apply_index(0, 3), 2);
    }
}
