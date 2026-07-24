/// Which palette the console renders in.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Theme {
    /// Whatever the machine is set to. The default, and the only one that keeps
    /// following the system when it flips at sunset — the other two are the
    /// reader overriding it.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// The class on the panel that wraps everything.
    ///
    /// `System` carries none: the stylesheet's own defaults follow
    /// `prefers-color-scheme`, and a class is only ever an override of that.
    pub fn class(&self) -> &'static str {
        match self {
            Theme::System => "",
            Theme::Light => "theme-light",
            Theme::Dark => "theme-dark",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Theme::System => "auto",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// One control cycling through all three, rather than three controls.
    pub fn next(&self) -> Self {
        match self {
            Theme::System => Theme::Light,
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::System,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycling_comes_back_to_where_it_started() {
        let theme = Theme::default();
        assert_eq!(theme, Theme::System);

        assert_eq!(theme.next().next().next(), Theme::System);
    }

    #[test]
    fn only_an_override_carries_a_class() {
        assert_eq!(Theme::System.class(), "");
        assert!(!Theme::Light.class().is_empty());
        assert!(!Theme::Dark.class().is_empty());
    }
}
