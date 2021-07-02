use crate::{heart::*, hooks::*, widgets::*};
use crate::macros::{rsx, widget};
use palette::Shade;
use stretch::{
    geometry::Size,
    style::{AlignItems, Dimension, FlexDirection, JustifyContent, PositionType, Style},
};

#[widget]
pub fn frame_counter(context: Context) -> Widget {
    let counter = context.listenable(0);

    let next_counter = context.listen(counter) + 1;
    context.shout(counter, next_counter);

    rsx! {
        <text>{format!("{:.3}", context.listen(counter))}</text>
    }
}

#[widget]
pub fn repro(context: Context, children: Widget) -> Widget {
    rsx!{
        <container>
            <container>
                {children.clone()}
            </container>
        </container>
    }
}


#[widget(on_click = (| context, clicked | {}), color = crate::theme::BG)]
pub fn button(
    on_click: impl Fn(Context, bool) -> () + Clone + Sync + Send + 'static,
    color: Color,
    children: Widget,
    context: Context,
) -> Widget {
    let clicked = context.listenable(false);

    let color = if context.listen(clicked) { color.lighten(0.1) } else { color };

    let callback = move |context: Context, is_clicked| {
        context.shout(clicked, is_clicked);
        if is_clicked {
            on_click(context, is_clicked);
        }
    };

    rsx! {
        <input on_click={callback.clone()}>
            <rounded_rect color={color}>
                <padding all=Dimension::Points(10.)>
                    {children.clone()}
                </padding>
            </rounded_rect>
        </input>
    }
}

#[widget]
pub fn slider_demo(context: Context) -> Widget {
    let slider_value = context.listenable(24.0);

    let style = Style { align_items: AlignItems::FlexEnd, ..Default::default() };
    rsx! {
        <column fill_parent=true align_items=AlignItems::Center>
            <min_size height=Dimension::Points(300.0) style=style>
                <text size={context.listen(slider_value)}>
                    {format!("{:.1} px", context.listen(slider_value))}
                </text>
            </min_size>
            <slider
                val={context.listen(slider_value)}
                on_change={move |context: Context, new_val| {
                    context.shout(slider_value, new_val)
                }}
                min=12.0 max=300.0
            />
        </column>
    }
}

#[widget(min = 0.0, max = 1.0, width = 500.0, slide_color = crate::theme::BG, knob_color = crate::theme::BG_LIGHT)]
pub fn slider(
    val: f32,
    on_change: impl Fn(Context, f32) -> () + Clone + Send + Sync + 'static,
    min: f32,
    max: f32,
    width: f32,
    slide_color: Color,
    knob_color: Color,
    context: Context,
) -> Widget {
    let clicked = context.listenable(false);
    let on_click = move |context: Context, is_clicked| context.shout(clicked, is_clicked);
    let click_start_val = context.listenable(val);

    let on_move = move |context: Context, position: Vec2| {
        let clicked_changed = context.listen_changed(clicked);
        let clicked = context.listen(clicked);
        /*
        let click_start_val = if clicked & clicked_changed {
            context.shout(click_start_val, position);
            position
        } else {
            context.listen(click_start_val)
        };
         */

        if clicked {
            //let position_delta = position - click_start_position;
            //let distance = (position.y - 10.).abs();
            //let distance_factor = if distance < 15.0 { 1. } else { 1. + distance / 50. };
            let new_val = (position.x / width * (max - min) + min).clamp(min, max);

            on_change(context, new_val);
        }
    };

    let slide_style = Style {
        size: Size { width: Dimension::Percent(1.0), height: Dimension::Points(5.0) },
        ..Default::default()
    };
    let handle_container_style = Style {
        position_type: PositionType::Absolute,
        position: stretch::geometry::Rect {
            top: Dimension::Points(0.0),
            start: Dimension::Percent((val - min) / (max - min)),
            ..Default::default()
        },
        ..Default::default()
    };
    let handle_input_style = Style {
        position: stretch::geometry::Rect { start: Dimension::Points(-10.), ..Default::default() },
        ..Default::default()
    };
    let handle_rect_style = Style {
        size: Size { width: Dimension::Points(20.0), height: Dimension::Points(20.0) },
        ..Default::default()
    };
    let top_style = Style {
        size: Size { width: Dimension::Points(width), height: Dimension::Points(20.) },
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Stretch,
        justify_content: JustifyContent::Center,
        ..Default::default()
    };
    rsx! {
         <input on_move={on_move.clone()} style=top_style>
            <rounded_rect style=slide_style color=slide_color />
            <container style=handle_container_style>
                <input on_click={on_click} style=handle_input_style>
                    <rounded_rect border_radius=10.0 style=handle_rect_style color=knob_color />
                </input>
            </container>
         </input>
    }
}


#[widget(initial_value = 1, step_size = 1)]
pub fn counter(initial_value: i32, step_size: i32, context: Context) -> Widget {
    let count = context.listenable(initial_value);

    rsx! {
         <row align_items=AlignItems::Center justify_content=JustifyContent::Center>
            <button on_click={move |context: Context, state| context.shout(count, context.listen(count) - 1)}>
                <text>{" - ".to_string()}</text>
            </button>
            <padding all=Dimension::Points(10.)>
                <text>{format!("{}", context.listen(count))}</text>
            </padding>
            <button on_click={move |context: Context, state| context.shout(count, context.listen(count) + 1)}>
                <text>{" + ".to_string()}</text>
            </button>
         </row>
    }
}
