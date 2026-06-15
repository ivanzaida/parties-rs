use lurq::{
  app::ctx::Ctx,
  components::{Column, Row},
  layout::layout_kind::ScrollState,
  node::{Element, dimension::Dimension},
};

#[derive(Clone, Copy)]
pub struct VirtualScrollConfig {
  pub overscan: f32,
  pub initial_viewport_height: f32,
  pub align_end_when_unmeasured: bool,
}

impl Default for VirtualScrollConfig {
  fn default() -> Self {
    Self {
      overscan: 600.0,
      initial_viewport_height: 720.0,
      align_end_when_unmeasured: false,
    }
  }
}

pub struct VirtualScrollItem<T> {
  pub key: String,
  pub estimated_height: f32,
  pub data: T,
}

pub fn virtual_scroll_column<T>(
  ctx: &mut Ctx,
  scroll_state: &ScrollState,
  items: impl IntoIterator<Item = VirtualScrollItem<T>>,
  config: VirtualScrollConfig,
  render_item: impl Fn(&mut Ctx, T) -> Element,
) -> Column {
  let items = items.into_iter().collect::<Vec<_>>();
  let total_height = items.iter().map(|item| item.estimated_height.max(0.0)).sum::<f32>();
  let viewport_height = scroll_state.viewport_height();
  let scroll_y = scroll_state.scroll_y();
  let measured = viewport_height > 0.0;
  let viewport_height = if measured {
    viewport_height
  } else {
    config.initial_viewport_height.max(1.0)
  };
  let overscan = config.overscan.max(0.0);
  let window_start = if measured {
    (scroll_y - overscan).max(0.0)
  } else if config.align_end_when_unmeasured {
    (total_height - viewport_height - overscan).max(0.0)
  } else {
    0.0
  };
  let window_end = if measured {
    scroll_y + viewport_height + overscan
  } else if config.align_end_when_unmeasured {
    total_height
  } else {
    viewport_height + overscan
  };

  let mut cursor = 0.0;
  let mut top_spacer = 0.0;
  let mut bottom_spacer = 0.0;
  let mut visible = Vec::new();

  for item in items {
    let item_height = item.estimated_height.max(0.0);
    let item_top = cursor;
    let item_bottom = item_top + item_height;
    cursor = item_bottom;

    if item_bottom < window_start {
      top_spacer = item_bottom;
    } else if item_top <= window_end {
      visible.push(item);
    } else {
      bottom_spacer += item_height;
    }
  }

  let rendered_items = ctx.for_each(
    visible,
    |item| item.key.clone(),
    move |ctx, item| {
      Column::new()
        .width(Dimension::Pct(100.0))
        .height(item.estimated_height.max(0.0))
        .child(render_item(ctx, item.data))
        .into()
    },
  );

  let mut children = Vec::with_capacity(rendered_items.len() + 2);
  children.push(virtual_spacer(top_spacer));
  children.extend(rendered_items);
  children.push(virtual_spacer(bottom_spacer));

  Column::new().width(Dimension::Pct(100.0)).with_children(children)
}

fn virtual_spacer(height: f32) -> Element {
  Row::new().width(Dimension::Pct(100.0)).height(height.max(0.0)).into()
}
