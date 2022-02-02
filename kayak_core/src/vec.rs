use derivative::*;

use crate::{context_ref::KayakContextRef, styles::Style, Index, Widget, Children, OnEvent, WidgetProps};

#[derive(Derivative)]
#[derivative(Default, Debug, PartialEq, Clone)]
pub struct VecTrackerProps<T> {
    pub data: Vec<T>,
    #[derivative(Default(value = "None"))]
    pub styles: Option<Style>,
    #[derivative(Default(value = "None"), Debug = "ignore", PartialEq = "ignore")]
    pub children: crate::Children,
    #[derivative(Default(value = "None"), Debug = "ignore", PartialEq = "ignore")]
    pub on_event: Option<crate::OnEvent>,
}

#[derive(Derivative)]
#[derivative(Debug, PartialEq, Clone, Default)]
pub struct VecTracker<T> {
    pub id: Index,
    pub props: VecTrackerProps<T>,
}

impl<T> VecTracker<T> {
    pub fn new(data: Vec<T>) -> Self {
        let props = VecTrackerProps {
            data,
            styles: None,
            children: None,
            on_event: None,
        };

        Self {
            id: Index::default(),
            props,
        }
    }
}

impl<T, I> From<I> for VecTracker<T>
    where
        I: Iterator<Item=T>,
{
    fn from(iter: I) -> Self {
        Self::new(iter.collect())
    }
}

impl<T> WidgetProps for VecTrackerProps<T>
    where
        T: Widget, {

    fn get_children(&self) -> Children {
        self.children.clone()
    }

    fn get_styles(&self) -> Option<Style> {
        self.styles.clone()
    }

    fn get_on_event(&self) -> Option<OnEvent> {
        self.on_event.clone()
    }

    fn get_focusable(&self) -> Option<bool> {
        Some(false)
    }
}

impl<T> Widget for VecTracker<T>
    where
        T: Widget,
{
    type Props = VecTrackerProps<T>;

    fn constructor(props: Self::Props) -> Self where Self: Sized {
        Self {
            id: Index::default(),
            props,
        }
    }

    fn get_id(&self) -> Index {
        self.id
    }

    fn set_id(&mut self, id: Index) {
        self.id = id;
    }

    fn get_props(&self) -> &Self::Props {
        &self.props
    }

    fn get_props_mut(&mut self) -> &mut Self::Props {
        &mut self.props
    }

    fn render(&mut self, context: &mut KayakContextRef) {
        for (index, item) in self.props.data.iter().enumerate() {
            context.add_widget(item.clone(), index);
        }

        context.commit();
    }
}
