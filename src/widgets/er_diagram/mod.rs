//! Static entity-relationship diagram widget.

mod layout;
mod node;
mod reconcile;
mod theme;

pub use layout::measure_er_diagram;
pub use node::ErDiagramNode;
pub use reconcile::reconcile_er_diagram;
pub use theme::ErDiagramTheme;

use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, Length, Padding, Style};
use std::sync::Arc;

/// Cardinality of one side of an [`ErRelation`], rendered as crow's-foot notation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ErCardinality {
    /// Zero or one (`|o`).
    ZeroOrOne,
    /// Exactly one (`||`).
    ExactlyOne,
    /// Zero or more (`}o`).
    ZeroOrMore,
    /// One or more (`}|`).
    #[default]
    OneOrMore,
}

/// A single column of an [`ErEntity`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ErAttribute {
    /// Attribute data type (e.g. `"int"`, `"varchar"`).
    pub ty: Arc<str>,
    /// Attribute name.
    pub name: Arc<str>,
    /// Whether this attribute is a primary key.
    pub pk: bool,
    /// Whether this attribute is a foreign key.
    pub fk: bool,
    /// Whether this attribute carries a unique key constraint.
    pub uk: bool,
}
impl ErAttribute {
    /// Creates an attribute with the given type and name (no key flags set).
    pub fn new(ty: impl Into<Arc<str>>, name: impl Into<Arc<str>>) -> Self {
        Self {
            ty: ty.into(),
            name: name.into(),
            pk: false,
            fk: false,
            uk: false,
        }
    }
    /// Marks this attribute as a primary key.
    pub fn pk(mut self) -> Self {
        self.pk = true;
        self
    }
    /// Marks this attribute as a foreign key.
    pub fn fk(mut self) -> Self {
        self.fk = true;
        self
    }
    /// Marks this attribute as a unique key.
    pub fn uk(mut self) -> Self {
        self.uk = true;
        self
    }
}

/// A table/entity in an [`ErDiagram`], with a name and ordered attributes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ErEntity {
    /// Entity (table) name.
    pub name: Arc<str>,
    /// Ordered list of attributes (columns).
    pub attributes: Vec<ErAttribute>,
}
impl ErEntity {
    /// Creates an entity with the given name and no attributes.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
        }
    }
    /// Appends an attribute and returns the entity for chaining.
    pub fn attribute(mut self, attribute: ErAttribute) -> Self {
        self.attributes.push(attribute);
        self
    }
}

/// A relationship between two entities, with crow's-foot cardinality on each end.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ErRelation {
    /// Name of the entity on the left side.
    pub left: Arc<str>,
    /// Name of the entity on the right side.
    pub right: Arc<str>,
    /// Cardinality at the left end.
    pub left_cardinality: ErCardinality,
    /// Cardinality at the right end.
    pub right_cardinality: ErCardinality,
    /// Optional label drawn on the relationship edge.
    pub label: Option<Arc<str>>,
}

/// A static entity-relationship diagram laid out automatically from entities and
/// relations. Build it with the chaining setters and convert into an [`Element`].
#[derive(Clone)]
pub struct ErDiagram {
    pub(crate) entities: Arc<[ErEntity]>,
    pub(crate) relations: Arc<[ErRelation]>,
    pub(crate) style: Style,
    pub(crate) entity_style: Style,
    pub(crate) edge_style: Style,
    pub(crate) label_style: Style,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) node_padding: Padding,
    pub(crate) layer_gap: u16,
    pub(crate) node_gap: u16,
    pub(crate) max_node_width: u16,
    pub(crate) theme: ErDiagramTheme,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for ErDiagram {
    fn default() -> Self {
        Self {
            entities: Arc::new([]),
            relations: Arc::new([]),
            style: Style::default(),
            entity_style: Style::default(),
            edge_style: Style::default(),
            label_style: Style::default(),
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            node_padding: (0, 1).into(),
            layer_gap: 4,
            node_gap: 4,
            max_node_width: 32,
            theme: ErDiagramTheme::default(),
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl ErDiagram {
    /// Creates an empty diagram with default styling.
    pub fn new() -> Self {
        Self::default()
    }
    /// Replaces the entity set with `entities`.
    pub fn entities(mut self, entities: impl IntoIterator<Item = ErEntity>) -> Self {
        self.entities = entities.into_iter().collect::<Vec<_>>().into();
        self
    }
    /// Replaces the relation set with `relations`.
    pub fn relations(mut self, relations: impl IntoIterator<Item = ErRelation>) -> Self {
        self.relations = relations.into_iter().collect::<Vec<_>>().into();
        self
    }
    /// Appends an entity by name (no attributes). See [`attribute`](Self::attribute)
    /// to add columns to it.
    pub fn entity(mut self, name: impl Into<Arc<str>>) -> Self {
        let mut v = self.entities.to_vec();
        v.push(ErEntity::new(name));
        self.entities = v.into();
        self
    }
    /// Adds an attribute to the named entity, creating the entity if it does not
    /// yet exist.
    pub fn attribute(
        mut self,
        entity: impl AsRef<str>,
        ty: impl Into<Arc<str>>,
        name: impl Into<Arc<str>>,
    ) -> Self {
        self.update_entity(entity.as_ref(), |e| {
            e.attributes.push(ErAttribute::new(ty, name))
        });
        self
    }
    /// Adds a relationship between two entities with the given cardinalities and
    /// optional edge label.
    pub fn relation(
        mut self,
        left: impl Into<Arc<str>>,
        right: impl Into<Arc<str>>,
        left_cardinality: ErCardinality,
        right_cardinality: ErCardinality,
        label: impl Into<Option<Arc<str>>>,
    ) -> Self {
        let mut v = self.relations.to_vec();
        v.push(ErRelation {
            left: left.into(),
            right: right.into(),
            left_cardinality,
            right_cardinality,
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
    /// Sets the style applied to entity (table) boxes.
    pub fn entity_style(mut self, style: Style) -> Self {
        self.entity_style = style;
        self
    }
    /// Sets the style applied to relationship edges.
    pub fn edge_style(mut self, style: Style) -> Self {
        self.edge_style = style;
        self
    }
    /// Sets the style applied to edge labels.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
    /// Sets the border line style for entity boxes.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.border_style = style;
        self
    }
    /// Sets the outer padding of the diagram.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
    /// Sets the inner padding of each entity box.
    pub fn node_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.node_padding = padding.into();
        self
    }
    /// Caps the rendered width of an entity box (minimum 1); longer content wraps
    /// or truncates.
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
    fn update_entity(&mut self, name: &str, f: impl FnOnce(&mut ErEntity)) {
        let mut entities = self.entities.to_vec();
        let index = entities
            .iter()
            .position(|e| e.name.as_ref() == name)
            .unwrap_or_else(|| {
                entities.push(ErEntity::new(name.to_owned()));
                entities.len() - 1
            });
        f(&mut entities[index]);
        self.entities = entities.into();
    }
}
impl From<ErDiagram> for Element {
    fn from(value: ErDiagram) -> Self {
        Element::new(ElementKind::ErDiagram(Box::new(value)))
    }
}
