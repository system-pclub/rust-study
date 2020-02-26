/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::geom::{flow_relative, PhysicalSides, PhysicalSize};
use style::properties::ComputedValues;
use style::values::computed::{Length, LengthPercentage, LengthPercentageOrAuto};
use style::values::computed::{NonNegativeLengthPercentage, Size};
use style::values::generics::length::MaxSize;
use style::values::specified::box_ as stylo;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Display {
    None,
    Contents,
    GeneratingBox(DisplayGeneratingBox),
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DisplayGeneratingBox {
    OutsideInside {
        outside: DisplayOutside,
        inside: DisplayInside,
        // list_item: bool,
    },
    // Layout-internal display types go here:
    // https://drafts.csswg.org/css-display-3/#layout-specific-display
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DisplayOutside {
    Block,
    Inline,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DisplayInside {
    Flow,
    FlowRoot,
}

pub(crate) trait ComputedValuesExt {
    fn inline_size_is_length(&self) -> bool;
    fn inline_box_offsets_are_both_non_auto(&self) -> bool;
    fn box_offsets(&self) -> flow_relative::Sides<LengthPercentageOrAuto>;
    fn box_size(&self) -> flow_relative::Vec2<LengthPercentageOrAuto>;
    fn min_box_size(&self) -> flow_relative::Vec2<LengthPercentageOrAuto>;
    fn max_box_size(&self) -> flow_relative::Vec2<MaxSize<LengthPercentage>>;
    fn padding(&self) -> flow_relative::Sides<LengthPercentage>;
    fn border_width(&self) -> flow_relative::Sides<Length>;
    fn margin(&self) -> flow_relative::Sides<LengthPercentageOrAuto>;
}

impl ComputedValuesExt for ComputedValues {
    fn inline_size_is_length(&self) -> bool {
        let position = self.get_position();
        let size = if self.writing_mode.is_horizontal() {
            &position.width
        } else {
            &position.height
        };
        matches!(size, Size::LengthPercentage(lp) if lp.0.to_length().is_some())
    }

    fn inline_box_offsets_are_both_non_auto(&self) -> bool {
        let position = self.get_position();
        let (a, b) = if self.writing_mode.is_horizontal() {
            (&position.left, &position.right)
        } else {
            (&position.top, &position.bottom)
        };
        !a.is_auto() && !b.is_auto()
    }

    #[inline]
    fn box_offsets(&self) -> flow_relative::Sides<LengthPercentageOrAuto> {
        let position = self.get_position();
        flow_relative::Sides::from_physical(
            &PhysicalSides::new(
                position.top.clone(),
                position.right.clone(),
                position.bottom.clone(),
                position.left.clone(),
            ),
            self.writing_mode,
        )
    }

    #[inline]
    fn box_size(&self) -> flow_relative::Vec2<LengthPercentageOrAuto> {
        let position = self.get_position();
        flow_relative::Vec2::from_physical_size(
            &PhysicalSize::new(
                size_to_length(position.width.clone()),
                size_to_length(position.height.clone()),
            ),
            self.writing_mode,
        )
    }

    #[inline]
    fn min_box_size(&self) -> flow_relative::Vec2<LengthPercentageOrAuto> {
        let position = self.get_position();
        flow_relative::Vec2::from_physical_size(
            &PhysicalSize::new(
                size_to_length(position.min_width.clone()),
                size_to_length(position.min_height.clone()),
            ),
            self.writing_mode,
        )
    }

    #[inline]
    fn max_box_size(&self) -> flow_relative::Vec2<MaxSize<LengthPercentage>> {
        let unwrap = |max_size: MaxSize<NonNegativeLengthPercentage>| match max_size {
            MaxSize::LengthPercentage(length) => MaxSize::LengthPercentage(length.0),
            MaxSize::None => MaxSize::None,
        };
        let position = self.get_position();
        flow_relative::Vec2::from_physical_size(
            &PhysicalSize::new(
                unwrap(position.max_width.clone()),
                unwrap(position.max_height.clone()),
            ),
            self.writing_mode,
        )
    }

    #[inline]
    fn padding(&self) -> flow_relative::Sides<LengthPercentage> {
        let padding = self.get_padding();
        flow_relative::Sides::from_physical(
            &PhysicalSides::new(
                padding.padding_top.0.clone(),
                padding.padding_right.0.clone(),
                padding.padding_bottom.0.clone(),
                padding.padding_left.0.clone(),
            ),
            self.writing_mode,
        )
    }

    fn border_width(&self) -> flow_relative::Sides<Length> {
        let border = self.get_border();
        flow_relative::Sides::from_physical(
            &PhysicalSides::new(
                border.border_top_width.0,
                border.border_right_width.0,
                border.border_bottom_width.0,
                border.border_left_width.0,
            ),
            self.writing_mode,
        )
    }

    fn margin(&self) -> flow_relative::Sides<LengthPercentageOrAuto> {
        let margin = self.get_margin();
        flow_relative::Sides::from_physical(
            &PhysicalSides::new(
                margin.margin_top.clone(),
                margin.margin_right.clone(),
                margin.margin_bottom.clone(),
                margin.margin_left.clone(),
            ),
            self.writing_mode,
        )
    }
}

impl From<stylo::Display> for Display {
    fn from(packed: stylo::Display) -> Self {
        let inside = match packed.inside() {
            stylo::DisplayInside::Flow => DisplayInside::Flow,
            stylo::DisplayInside::FlowRoot => DisplayInside::FlowRoot,

            // These should not be values of DisplayInside, but oh well
            stylo::DisplayInside::None => return Display::None,
            stylo::DisplayInside::Contents => return Display::Contents,
        };
        let outside = match packed.outside() {
            stylo::DisplayOutside::Block => DisplayOutside::Block,
            stylo::DisplayOutside::Inline => DisplayOutside::Inline,

            // This should not be a value of DisplayInside, but oh well
            stylo::DisplayOutside::None => return Display::None,
        };
        Display::GeneratingBox(DisplayGeneratingBox::OutsideInside {
            outside,
            inside,
            // list_item: packed.is_list_item(),
        })
    }
}

fn size_to_length(size: Size) -> LengthPercentageOrAuto {
    match size {
        Size::LengthPercentage(length) => {
            LengthPercentageOrAuto::LengthPercentage(length.0.clone())
        },
        Size::Auto => LengthPercentageOrAuto::Auto,
    }
}
