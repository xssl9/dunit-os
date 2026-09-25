//! The concrete widget set and its mapping from DUI element tags.

/// A concrete UI widget. Container kinds (`Row`/`Column`/...) are *not* widgets;
/// they are handled by [`dunit_ui`] layout. Every widget corresponds to a
/// non-container [`dunit_ui::Kind::Element`] tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widget {
    /// A run of text.
    Text,
    /// A single glyph/icon sized to the font.
    Icon,
    /// A clickable button.
    Button,
    /// A single-line text input.
    Input,
    /// A vertical list of selectable rows.
    List,
    /// A menu of selectable items.
    Menu,
    /// A draggable value slider.
    Slider,
    /// A progress indicator.
    Progress,
    /// A transient notification banner.
    Notification,
}

impl Widget {
    /// Map an element tag to a widget, if it names one. Matching is
    /// case-sensitive on the canonical capitalised tag (as DUI emits).
    pub fn from_tag(tag: &str) -> Option<Widget> {
        Some(match tag {
            "Text" | "Label" => Widget::Text,
            "Icon" => Widget::Icon,
            "Button" => Widget::Button,
            "Input" | "TextInput" => Widget::Input,
            "List" => Widget::List,
            "Menu" => Widget::Menu,
            "Slider" => Widget::Slider,
            "Progress" => Widget::Progress,
            "Notification" | "Toast" => Widget::Notification,
            _ => return None,
        })
    }

    /// The canonical element tag for this widget.
    pub fn tag(self) -> &'static str {
        match self {
            Widget::Text => "Text",
            Widget::Icon => "Icon",
            Widget::Button => "Button",
            Widget::Input => "Input",
            Widget::List => "List",
            Widget::Menu => "Menu",
            Widget::Slider => "Slider",
            Widget::Progress => "Progress",
            Widget::Notification => "Notification",
        }
    }

    /// Whether the widget can hold keyboard focus (Tab reaches it).
    pub fn focusable(self) -> bool {
        matches!(
            self,
            Widget::Button | Widget::Input | Widget::List | Widget::Menu | Widget::Slider
        )
    }

    /// Whether the widget reacts to pointer interaction (hover/press).
    pub fn interactive(self) -> bool {
        self.focusable()
    }

    /// Whether the widget carries a mutable value (text, fraction or selection).
    pub fn has_value(self) -> bool {
        matches!(
            self,
            Widget::Input | Widget::Slider | Widget::Progress | Widget::List | Widget::Menu
        )
    }
}
