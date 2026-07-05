//! Static UML class diagram widget.

mod layout;
mod node;
mod reconcile;
mod theme;

pub use layout::measure_class_diagram;
pub use node::ClassDiagramNode;
pub use reconcile::reconcile_class_diagram;
pub use theme::ClassDiagramTheme;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, Length, Padding, Style};

/// UML visibility of a [`ClassMember`], rendered as a `+ - # ~` prefix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClassVisibility {
    /// Public (`+`).
    #[default]
    Public,
    /// Private (`-`).
    Private,
    /// Protected (`#`).
    Protected,
    /// Package/internal (`~`).
    Package,
}

/// The kind of a [`ClassRelation`], controlling the edge/arrowhead style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClassRelationKind {
    /// Plain association (solid line).
    #[default]
    Association,
    /// Dependency (dashed line, open arrow).
    Dependency,
    /// Inheritance / generalization (solid line, hollow triangle).
    Inheritance,
    /// Interface realization (dashed line, hollow triangle).
    Realization,
    /// Composition (filled diamond).
    Composition,
    /// Aggregation (hollow diamond).
    Aggregation,
}

/// A single attribute or method of a [`ClassSpec`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClassMember {
    /// Visibility prefix.
    pub visibility: ClassVisibility,
    /// Member name.
    pub name: Arc<str>,
    /// Type (for attributes) or signature (for methods), if any.
    pub ty: Option<Arc<str>>,
}

/// A class box with its attributes and methods.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClassSpec {
    /// Class name.
    pub name: Arc<str>,
    /// Attribute (field) members.
    pub attributes: Vec<ClassMember>,
    /// Method members.
    pub methods: Vec<ClassMember>,
}

/// A relationship between two classes, with kind, multiplicities, and label.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClassRelation {
    /// Source class name.
    pub from: Arc<str>,
    /// Target class name.
    pub to: Arc<str>,
    /// Relationship kind.
    pub kind: ClassRelationKind,
    /// Optional multiplicity shown at the `from` end.
    pub multiplicity_from: Option<Arc<str>>,
    /// Optional multiplicity shown at the `to` end.
    pub multiplicity_to: Option<Arc<str>>,
    /// Optional edge label.
    pub label: Option<Arc<str>>,
}

/// A static UML class diagram laid out automatically from classes and relations.
/// Build it with the chaining setters and convert into an [`Element`].
#[derive(Clone)]
pub struct ClassDiagram {
    pub(crate) classes: Arc<[ClassSpec]>,
    pub(crate) relations: Arc<[ClassRelation]>,
    pub(crate) style: Style,
    pub(crate) class_style: Style,
    pub(crate) edge_style: Style,
    pub(crate) label_style: Style,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) node_padding: Padding,
    pub(crate) layer_gap: u16,
    pub(crate) node_gap: u16,
    pub(crate) max_node_width: u16,
    pub(crate) theme: ClassDiagramTheme,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for ClassDiagram {
    fn default() -> Self {
        Self {
            classes: Arc::new([]),
            relations: Arc::new([]),
            style: Style::default(),
            class_style: Style::default(),
            edge_style: Style::default(),
            label_style: Style::default(),
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            node_padding: (0, 1).into(),
            layer_gap: 4,
            node_gap: 4,
            max_node_width: 32,
            theme: ClassDiagramTheme::default(),
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl ClassDiagram {
    /// Creates an empty diagram with default styling.
    pub fn new() -> Self {
        Self::default()
    }
    /// Replaces the class set with `classes`.
    pub fn classes(mut self, classes: impl IntoIterator<Item = ClassSpec>) -> Self {
        self.classes = classes.into_iter().collect::<Vec<_>>().into();
        self
    }
    /// Replaces the relation set with `relations`.
    pub fn relations(mut self, relations: impl IntoIterator<Item = ClassRelation>) -> Self {
        self.relations = relations.into_iter().collect::<Vec<_>>().into();
        self
    }
    /// Appends an empty class by name. See [`attribute`](Self::attribute) and
    /// [`method`](Self::method) to add members.
    pub fn class(mut self, name: impl Into<Arc<str>>) -> Self {
        let mut v = self.classes.to_vec();
        v.push(ClassSpec::new(name));
        self.classes = v.into();
        self
    }
    /// Adds an attribute to the named class, creating the class if it does not
    /// yet exist.
    pub fn attribute(
        mut self,
        class: impl AsRef<str>,
        visibility: ClassVisibility,
        name: impl Into<Arc<str>>,
        ty: impl Into<Arc<str>>,
    ) -> Self {
        self.update_class(class.as_ref(), |c| {
            c.attributes.push(ClassMember {
                visibility,
                name: name.into(),
                ty: Some(ty.into()),
            })
        });
        self
    }
    /// Adds a method to the named class, creating the class if it does not yet
    /// exist.
    pub fn method(
        mut self,
        class: impl AsRef<str>,
        visibility: ClassVisibility,
        name: impl Into<Arc<str>>,
        sig: impl Into<Arc<str>>,
    ) -> Self {
        self.update_class(class.as_ref(), |c| {
            c.methods.push(ClassMember {
                visibility,
                name: name.into(),
                ty: Some(sig.into()),
            })
        });
        self
    }
    /// Adds a relationship between two classes with the given kind, multiplicities,
    /// and optional label.
    pub fn relation(
        mut self,
        from: impl Into<Arc<str>>,
        to: impl Into<Arc<str>>,
        kind: ClassRelationKind,
        multiplicity_from: impl Into<Option<Arc<str>>>,
        multiplicity_to: impl Into<Option<Arc<str>>>,
        label: impl Into<Option<Arc<str>>>,
    ) -> Self {
        let mut v = self.relations.to_vec();
        v.push(ClassRelation {
            from: from.into(),
            to: to.into(),
            kind,
            multiplicity_from: multiplicity_from.into(),
            multiplicity_to: multiplicity_to.into(),
            label: label.into(),
        });
        self.relations = v.into();
        self
    }
    /// Sets the base style of the diagram container.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    /// Sets the style applied to class boxes.
    pub fn class_style(mut self, style: Style) -> Self {
        self.class_style = style;
        self
    }
    /// Sets the style applied to relation edges.
    pub fn edge_style(mut self, style: Style) -> Self {
        self.edge_style = style;
        self
    }
    /// Sets the style applied to edge labels.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
    /// Sets the border line style for class boxes.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.border_style = style;
        self
    }
    /// Sets the outer padding of the diagram.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
    /// Sets the inner padding of each class box.
    pub fn node_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.node_padding = padding.into();
        self
    }
    /// Caps the rendered width of a class box (minimum 1).
    pub fn max_node_width(mut self, width: u16) -> Self {
        self.max_node_width = width.max(1);
        self
    }
    /// Sets the width of the diagram container.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }
    /// Sets the height of the diagram container.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    fn update_class(&mut self, name: &str, f: impl FnOnce(&mut ClassSpec)) {
        let mut classes = self.classes.to_vec();
        let index = classes
            .iter()
            .position(|c| c.name.as_ref() == name)
            .unwrap_or_else(|| {
                classes.push(ClassSpec::new(name.to_owned()));
                classes.len() - 1
            });
        f(&mut classes[index]);
        self.classes = classes.into();
    }
}

impl ClassSpec {
    /// Creates a class with the given name and no members.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
            methods: Vec::new(),
        }
    }
    /// Appends an attribute member and returns the spec for chaining.
    pub fn attribute(
        mut self,
        visibility: ClassVisibility,
        name: impl Into<Arc<str>>,
        ty: impl Into<Arc<str>>,
    ) -> Self {
        self.attributes.push(ClassMember {
            visibility,
            name: name.into(),
            ty: Some(ty.into()),
        });
        self
    }
    /// Appends a method member and returns the spec for chaining.
    pub fn method(
        mut self,
        visibility: ClassVisibility,
        name: impl Into<Arc<str>>,
        sig: impl Into<Arc<str>>,
    ) -> Self {
        self.methods.push(ClassMember {
            visibility,
            name: name.into(),
            ty: Some(sig.into()),
        });
        self
    }
}

impl From<ClassDiagram> for Element {
    fn from(value: ClassDiagram) -> Self {
        Element::new(ElementKind::ClassDiagram(Box::new(value)))
    }
}
