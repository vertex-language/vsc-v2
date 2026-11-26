//! Homogeneous aggregate classification helpers (`rustc_abi::callconv`).

use crate::{BackendRepr, FieldsShape, Size, TyAbiInterface, TyAndLayout, Variants};

mod reg;

pub use reg::{Reg, RegKind};

#[derive(Copy, Clone, Debug)]
pub enum HomogeneousAggregate {
    /// All leaf fields are passed in the same register kind and size.
    Homogeneous(Reg),
    /// No leaf fields at all.
    NoData,
}

#[derive(Copy, Clone, Debug)]
pub struct Heterogeneous;

impl HomogeneousAggregate {
    pub fn unit(self) -> Option<Reg> {
        match self {
            HomogeneousAggregate::Homogeneous(reg) => Some(reg),
            HomogeneousAggregate::NoData => None,
        }
    }

    fn merge(self, other: HomogeneousAggregate) -> Result<HomogeneousAggregate, Heterogeneous> {
        match (self, other) {
            (x, HomogeneousAggregate::NoData) | (HomogeneousAggregate::NoData, x) => Ok(x),
            (HomogeneousAggregate::Homogeneous(a), HomogeneousAggregate::Homogeneous(b)) => {
                if a != b {
                    return Err(Heterogeneous);
                }
                Ok(self)
            }
        }
    }
}

impl<'a, Ty> TyAndLayout<'a, Ty> {
    /// Returns `Homogeneous` if this layout is an aggregate containing fields
    /// of only a single scalar/vector register type, tightly packed or overlaid (union).
    pub fn homogeneous_aggregate<C>(&self, cx: &C) -> Result<HomogeneousAggregate, Heterogeneous>
    where
        Ty: TyAbiInterface<'a, C> + Copy,
    {
        match self.backend_repr {
            BackendRepr::Scalar(scalar) => {
                let kind = RegKind::from_primitive(scalar.primitive());
                Ok(HomogeneousAggregate::Homogeneous(Reg { kind, size: self.size }))
            }
            BackendRepr::SimdVector { element, count: _ } => {
                assert!(!self.is_zst());
                Ok(HomogeneousAggregate::Homogeneous(Reg {
                    kind: RegKind::Vector { hint_vector_elem: element.primitive() },
                    size: self.size,
                }))
            }
            BackendRepr::ScalarPair { .. } | BackendRepr::Memory { sized: true } => {
                let from_fields_at = |layout: Self,
                                      start: Size|
                 -> Result<(HomogeneousAggregate, Size), Heterogeneous> {
                    let is_union = matches!(layout.fields, FieldsShape::Union(_));
                    let mut result = HomogeneousAggregate::NoData;
                    let mut cumulative_size = start;

                    if layout.is_zst() {
                        if layout.align.bytes() == 1 {
                            return Ok((HomogeneousAggregate::NoData, start));
                        } else {
                            return Err(Heterogeneous);
                        }
                    }

                    match layout.variants {
                        Variants::Single { .. } => {}
                        Variants::Empty => return Err(Heterogeneous),
                        Variants::Multiple { .. } => {
                            let tag_result = layout
                                .field(cx, 0)
                                .homogeneous_aggregate(cx)?;
                            if let Some(unit) = tag_result.unit() {
                                if unit.size != layout.size {
                                    return Err(Heterogeneous);
                                }
                            }
                            return Ok((tag_result, start + layout.size));
                        }
                    }

                    for i in 0..layout.fields.count() {
                        let field = layout.field(cx, i);
                        let off = start + layout.fields.offset(i);
                        if !is_union && cumulative_size != off {
                            if !(field.is_1zst()) {
                                return Err(Heterogeneous);
                            }
                        }
                        let field_ha = field.homogeneous_aggregate(cx)?;
                        result = result.merge(field_ha)?;
                        let end = off + field.size;
                        if !is_union {
                            cumulative_size = end;
                        } else {
                            cumulative_size = cumulative_size.max(end);
                        }
                    }

                    Ok((result, cumulative_size))
                };

                let (ha, total) = from_fields_at(*self, Size::ZERO)?;
                if total != self.size && !matches!(self.fields, FieldsShape::Union(_)) {
                    return Err(Heterogeneous);
                }
                Ok(ha)
            }
            BackendRepr::Memory { sized: false } => Err(Heterogeneous),
        }
    }
}
