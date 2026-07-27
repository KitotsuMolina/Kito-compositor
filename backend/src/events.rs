use crate::Output;

#[derive(Debug, Default)]
pub struct EventTracker {
    outputs: Option<Vec<OutputIdentity>>,
    focus: Option<Option<String>>,
}

impl EventTracker {
    pub fn observe_outputs(&mut self, outputs: &[Output]) -> bool {
        let mut current = outputs.iter().map(OutputIdentity::from).collect::<Vec<_>>();
        current.sort_by(|left, right| left.name.cmp(&right.name));
        let changed = self.outputs.as_ref() != Some(&current);
        self.outputs = Some(current);
        changed
    }

    pub fn observe_focus(&mut self, output: Option<&Output>) -> bool {
        let current = output.map(|value| value.name.clone());
        let changed = self.focus.as_ref() != Some(&current);
        self.focus = Some(current);
        changed
    }
}

#[derive(Debug, Clone, PartialEq)]
struct OutputIdentity {
    name: String,
    make: Option<String>,
    model: Option<String>,
    serial: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    scale: Option<f64>,
    refresh_hz: Option<f64>,
    active: bool,
}

impl From<&Output> for OutputIdentity {
    fn from(output: &Output) -> Self {
        Self {
            name: output.name.clone(),
            make: output.make.clone(),
            model: output.model.clone(),
            serial: output.serial.clone(),
            width: output.width,
            height: output.height,
            scale: output.scale,
            refresh_hz: output.refresh_hz,
            active: output.active,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, focused: bool) -> Output {
        Output {
            name: name.into(),
            make: None,
            model: None,
            serial: None,
            width: Some(1920),
            height: Some(1080),
            scale: Some(1.0),
            refresh_hz: Some(60.0),
            focused,
            active: true,
            backend: "fake".into(),
        }
    }

    #[test]
    fn output_events_ignore_focus_only_changes() {
        let mut tracker = EventTracker::default();
        assert!(tracker.observe_outputs(&[output("DP-1", false)]));
        assert!(!tracker.observe_outputs(&[output("DP-1", true)]));
        assert!(tracker.observe_outputs(&[output("HDMI-A-1", true)]));
    }

    #[test]
    fn focus_events_compare_only_the_output_name() {
        let mut tracker = EventTracker::default();
        let first = output("DP-1", true);
        let updated = Output {
            width: Some(2560),
            ..first.clone()
        };
        assert!(tracker.observe_focus(Some(&first)));
        assert!(!tracker.observe_focus(Some(&updated)));
        assert!(tracker.observe_focus(None));
    }
}
