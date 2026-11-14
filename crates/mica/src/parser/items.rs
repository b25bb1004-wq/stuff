//! Top-level items: types, constants, layers, functions, tests and properties.
use super::P;
use crate::ast::{
    ConstDef, Field, FnBody, FnDef, Item, LayerDef, Param, PropDef, TestDef, TypeBody, TypeDef,
    Variant,
};
use crate::lexer::Tok;
use crate::span::Diag;
use alloc::vec::Vec;

impl P {
    pub(super) fn item(&mut self) -> Result<Item, Diag> {
        match self.tok() {
            Tok::Type => self.type_def().map(Item::Type),
            Tok::Const => self.const_def().map(Item::Const),
            Tok::Layer => {
                let start = self.bump().span;
                let name = self.ident_l("layer name")?;
                Ok(Item::Layer(LayerDef {
                    name,
                    span: self.since(start),
                }))
            }
            Tok::Fn => self.fn_def().map(Item::Fn),
            Tok::Test => self.test_def().map(Item::Test),
            Tok::Prop => self.prop_def().map(Item::Prop),
            _ => Err(self.expected("'type', 'const', 'layer', 'fn', 'test' or 'prop'")),
        }
    }

    fn type_def(&mut self) -> Result<TypeDef, Diag> {
        let start = self.bump().span;
        let name = self.ident_u("type name")?;
        self.expect(&Tok::Eq, "'=' after the type name")?;
        let body = if self.eat(&Tok::LBrace) {
            let fields = self.normal(|p| {
                p.list(
                    &Tok::RBrace,
                    "'}' to close the record",
                    "field name",
                    true,
                    false,
                    |p| p.field(),
                )
            })?;
            TypeBody::Record(fields)
        } else {
            let mut variants: Vec<Variant> = Vec::new();
            variants.push(self.variant()?);
            while self.eat(&Tok::Bar) {
                variants.push(self.variant()?);
            }
            TypeBody::Variants(variants)
        };
        Ok(TypeDef {
            name,
            body,
            span: self.since(start),
        })
    }

    fn field(&mut self) -> Result<Field, Diag> {
        let start = self.span();
        let name = self.ident_l("field name")?;
        self.expect(&Tok::Colon, "':' after the field name")?;
        let ty = self.ty()?;
        Ok(Field {
            name,
            ty,
            span: self.since(start),
        })
    }

    fn variant(&mut self) -> Result<Variant, Diag> {
        let start = self.span();
        let name = self.ident_u("variant name")?;
        let fields = if self.eat(&Tok::LParen) {
            self.list(
                &Tok::RParen,
                "')' to close the variant",
                "type",
                false,
                false,
                |p| p.ty(),
            )?
        } else {
            Vec::new()
        };
        Ok(Variant {
            name,
            fields,
            span: self.since(start),
        })
    }

    fn const_def(&mut self) -> Result<ConstDef, Diag> {
        let start = self.bump().span;
        let name = self.ident_l("constant name")?;
        self.expect(&Tok::Colon, "':' after the constant name")?;
        let ty = self.ty()?;
        self.expect(&Tok::Eq, "'=' before the constant value")?;
        let value = self.expr()?;
        Ok(ConstDef {
            name,
            ty,
            value,
            span: self.since(start),
        })
    }

    pub(super) fn param(&mut self) -> Result<Param, Diag> {
        let start = self.span();
        let name = self.ident_l("parameter name")?;
        self.expect(&Tok::Colon, "':' after the parameter name")?;
        let ty = self.ty()?;
        Ok(Param {
            name,
            ty,
            span: self.since(start),
        })
    }

    fn fn_def(&mut self) -> Result<FnDef, Diag> {
        let start = self.bump().span;
        let name = self.ident_l("function name")?;
        self.expect(&Tok::LParen, "'(' after the function name")?;
        let params = self.list(
            &Tok::RParen,
            "')' to close the parameters",
            "parameter name",
            false,
            true,
            |p| p.param(),
        )?;
        let ret = if self.eat(&Tok::Arrow) {
            Some(self.ty()?)
        } else {
            None
        };
        let uses = self.uses_opt()?;
        let body = if self.eat(&Tok::Eq) {
            self.expect(&Tok::Extern, "'extern' after '='")?;
            let (path, span) = self.string_lit("the extern import name")?;
            FnBody::Extern { path, span }
        } else {
            FnBody::Block(self.block()?)
        };
        Ok(FnDef {
            name,
            params,
            ret,
            uses,
            body,
            span: self.since(start),
        })
    }

    fn test_def(&mut self) -> Result<TestDef, Diag> {
        let start = self.bump().span;
        let (name, _) = self.string_lit("the test name string")?;
        let body = self.block()?;
        Ok(TestDef {
            name,
            body,
            span: self.since(start),
        })
    }

    fn prop_def(&mut self) -> Result<PropDef, Diag> {
        let start = self.bump().span;
        let (name, _) = self.string_lit("the property name string")?;
        self.expect(&Tok::LParen, "'(' before the property parameters")?;
        let params = self.list(
            &Tok::RParen,
            "')' to close the parameters",
            "parameter name",
            false,
            false,
            |p| p.param(),
        )?;
        let cond = if self.eat(&Tok::Where) {
            Some(self.head_expr()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(PropDef {
            name,
            params,
            cond,
            body,
            span: self.since(start),
        })
    }
}
