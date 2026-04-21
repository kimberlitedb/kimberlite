//! PDF report generation for compliance frameworks.
//!
//! Migrated to `printpdf` 0.9 (v0.6.0 Tier 3 #10b). The 0.9 API replaces the
//! 0.7 document-construction model (`PdfDocument::new(...) -> (doc, page, layer)`
//! with `use_text` on a `PdfLayerReference`) with a layout-tree / operation-list
//! model: each `PdfPage` carries a `Vec<Op>` and the document is serialised via
//! `PdfDocument::save(&PdfSaveOptions, &mut Vec<PdfWarnMsg>) -> Vec<u8>`.
//!
//! The output contract (SOC 2 / HIPAA PDF artefacts) is structurally identical
//! to the 0.7 output: same sections, same headings, same per-requirement
//! entries, same footer, same A4 page size, same Helvetica/Helvetica-Bold
//! builtin fonts, same margins and line heights. Two incidental differences:
//!
//! * Status glyphs `✓` / `✗` are now rendered as `[OK]` / `[X]`. The 0.9
//!   builtin-font subsets are Win-1252-only (`ParsedFont::from_bytes` over the
//!   subsetted `Helvetica.subset.ttf`) and do not carry U+2713 / U+2717. ASCII
//!   markers preserve the semantic check/cross mark without missing glyphs.
//! * Byte layout differs from 0.7 (different serialiser, optimised streams) —
//!   there are no golden-file fixtures to rebaseline; the existing test
//!   asserts only the `%PDF` header and non-emptiness, which still holds.

use crate::{ComplianceFramework, ComplianceReport, Result};
use printpdf::{
    BuiltinFont, Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point,
    Pt, TextItem,
};

const FONT_SIZE_TITLE: f32 = 24.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const FONT_SIZE_SUBHEADING: f32 = 12.0;
const FONT_SIZE_BODY: f32 = 10.0;
const MARGIN: f32 = 50.0;
const LINE_HEIGHT: f32 = 14.0;
const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;

/// Cached builtin-font handles. Both are resolved once in `generate_pdf` so the
/// per-section helpers can issue `Op::SetFont` without replaying the lookup.
///
/// Under 0.7 these were `IndirectFontRef`s obtained from
/// `doc.add_builtin_font(BuiltinFont::Helvetica)`. Under 0.9 builtin fonts are
/// addressed via `PdfFontHandle::Builtin(BuiltinFont::Helvetica)` directly on
/// each `Op::SetFont` — no registration step is required for the 14 standard
/// Type 1 fonts. We *also* register a `ParsedFont` via `doc.add_font(..)` so
/// `PdfSaveOptions::subset_fonts` has something to subset.
#[derive(Clone)]
struct Fonts {
    regular: PdfFontHandle,
    bold: PdfFontHandle,
}

impl Fonts {
    fn builtin() -> Self {
        Self {
            regular: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
            bold: PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
        }
    }
}

/// Emit a single line of text at `(MARGIN + indent, y)` in the given font/size.
///
/// Mirrors the 0.7 `layer.use_text(text, size, Mm(x), Mm(y), &font)` one-shot
/// API. Under 0.9 each text emission is a small op sequence — we wrap with
/// `StartTextSection` / `EndTextSection` so positioning resets between calls
/// and `SetTextCursor` behaves as absolute placement (not relative to the
/// previous emission).
fn emit_text(
    ops: &mut Vec<Op>,
    text: impl Into<String>,
    size_pt: f32,
    x_mm: f32,
    y_mm: f32,
    font: &PdfFontHandle,
) {
    ops.push(Op::StartTextSection);
    ops.push(Op::SetFont {
        font: font.clone(),
        size: Pt(size_pt),
    });
    ops.push(Op::SetTextCursor {
        pos: Point::new(Mm(x_mm), Mm(y_mm)),
    });
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(text.into())],
    });
    ops.push(Op::EndTextSection);
}

/// Render the title and framework details section.
fn render_header(ops: &mut Vec<Op>, report: &ComplianceReport, fonts: &Fonts, y: &mut f32) {
    emit_text(
        ops,
        format!("{} Compliance Report", report.framework),
        FONT_SIZE_TITLE,
        MARGIN,
        *y,
        &fonts.bold,
    );
    *y -= LINE_HEIGHT * 2.0;

    emit_text(
        ops,
        format!("Framework: {}", report.framework.full_name()),
        FONT_SIZE_BODY,
        MARGIN,
        *y,
        &fonts.regular,
    );
    *y -= LINE_HEIGHT;

    emit_text(
        ops,
        format!(
            "Generated: {}",
            report.generated_at.format("%Y-%m-%d %H:%M:%S UTC")
        ),
        FONT_SIZE_BODY,
        MARGIN,
        *y,
        &fonts.regular,
    );
    *y -= LINE_HEIGHT * 2.0;
}

/// Render the verification summary section.
fn render_verification_summary(
    ops: &mut Vec<Op>,
    report: &ComplianceReport,
    fonts: &Fonts,
    y: &mut f32,
) {
    emit_text(
        ops,
        "Verification Summary",
        FONT_SIZE_HEADING,
        MARGIN,
        *y,
        &fonts.bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    emit_text(
        ops,
        format!(
            "Status: {} of {} requirements verified ({:.1}%)",
            report.certificate.verified_count,
            report.certificate.total_requirements,
            report.certificate.verification_percentage()
        ),
        FONT_SIZE_BODY,
        MARGIN,
        *y,
        &fonts.regular,
    );
    *y -= LINE_HEIGHT;

    emit_text(
        ops,
        format!("Toolchain: {}", report.certificate.toolchain_version),
        FONT_SIZE_BODY,
        MARGIN,
        *y,
        &fonts.regular,
    );
    *y -= LINE_HEIGHT;

    emit_text(
        ops,
        format!("Specification Hash: {}", report.certificate.spec_hash),
        FONT_SIZE_BODY,
        MARGIN,
        *y,
        &fonts.regular,
    );
    *y -= LINE_HEIGHT * 2.0;
}

/// Render the core compliance properties section.
fn render_core_properties(
    ops: &mut Vec<Op>,
    report: &ComplianceReport,
    fonts: &Fonts,
    y: &mut f32,
) {
    emit_text(
        ops,
        "Core Compliance Properties",
        FONT_SIZE_HEADING,
        MARGIN,
        *y,
        &fonts.bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    for (property, status) in &report.core_properties {
        // See module docs: builtin Helvetica subset is Win-1252; U+2713/U+2717
        // are not encodable. `[OK]` / `[X]` preserve the semantic check mark.
        let status_str = if *status {
            "[OK] Satisfied"
        } else {
            "[X] Not satisfied"
        };
        emit_text(
            ops,
            format!("{property}: {status_str}"),
            FONT_SIZE_BODY,
            MARGIN + 10.0,
            *y,
            &fonts.regular,
        );
        *y -= LINE_HEIGHT;
    }
    *y -= LINE_HEIGHT;
}

/// Render SLA metrics section (SOC 2 reports).
fn render_sla_metrics(ops: &mut Vec<Op>, fonts: &Fonts, y: &mut f32) {
    emit_text(
        ops,
        "SLA Metrics (SOC 2 A1.2 / CC7.4)",
        FONT_SIZE_HEADING,
        MARGIN,
        *y,
        &fonts.bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    let metrics = [
        ("Availability Target", "99.99% uptime SLA"),
        (
            "Recovery Point Objective (RPO)",
            "0 (append-only log, no data loss)",
        ),
        (
            "Recovery Time Objective (RTO)",
            "< 30s (state reconstruction from log)",
        ),
        (
            "Backup Verification",
            "Hash chain integrity check on every restore",
        ),
        (
            "Incident Response Time",
            "72h notification deadline (automated tracking)",
        ),
    ];

    for (metric, value) in &metrics {
        emit_text(
            ops,
            format!("{metric}: {value}"),
            FONT_SIZE_BODY,
            MARGIN + 10.0,
            *y,
            &fonts.regular,
        );
        *y -= LINE_HEIGHT;
    }
    *y -= LINE_HEIGHT;
}

/// Render security metrics section (ISO 27001 reports).
fn render_security_metrics(ops: &mut Vec<Op>, fonts: &Fonts, y: &mut f32) {
    emit_text(
        ops,
        "Security Metrics Summary (ISO 27001 A.12.4)",
        FONT_SIZE_HEADING,
        MARGIN,
        *y,
        &fonts.bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    let metrics = [
        ("Encryption Algorithm", "AES-256-GCM (FIPS 140-2 validated)"),
        (
            "Hash Chain Algorithm",
            "SHA-256 (compliance) / BLAKE3 (internal)",
        ),
        ("Signature Algorithm", "Ed25519 (per-tenant and per-record)"),
        (
            "Access Control Model",
            "RBAC (4 roles) + ABAC (19 policies)",
        ),
        (
            "Audit Log Integrity",
            "Immutable append-only with hash chain verification",
        ),
        (
            "Tenant Isolation",
            "Per-tenant encryption keys, placement routing",
        ),
    ];

    for (metric, value) in &metrics {
        emit_text(
            ops,
            format!("{metric}: {value}"),
            FONT_SIZE_BODY,
            MARGIN + 10.0,
            *y,
            &fonts.regular,
        );
        *y -= LINE_HEIGHT;
    }
    *y -= LINE_HEIGHT;
}

/// Render framework-specific metrics (SOC 2 SLA, ISO 27001 security).
fn render_framework_metrics(
    ops: &mut Vec<Op>,
    framework: ComplianceFramework,
    fonts: &Fonts,
    y: &mut f32,
) {
    match framework {
        ComplianceFramework::SOC2 => render_sla_metrics(ops, fonts, y),
        ComplianceFramework::ISO27001 => render_security_metrics(ops, fonts, y),
        _ => {}
    }
}

/// Pagination helper. When the current `y` crosses the bottom margin, finalise
/// the current page's ops into the `pages` vec and start a fresh page at the
/// top.
///
/// Returns the new `y` position (at top of page if paginated; unchanged
/// otherwise).
fn maybe_paginate(pages: &mut Vec<PdfPage>, ops: &mut Vec<Op>, y: f32) -> f32 {
    if y < MARGIN + 50.0 {
        // Finalise current page
        let page_ops = std::mem::take(ops);
        pages.push(PdfPage::new(
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            page_ops,
        ));
        PAGE_HEIGHT_MM - MARGIN
    } else {
        y
    }
}

/// Render the individual requirement entries with multi-page support.
fn render_requirements(
    pages: &mut Vec<PdfPage>,
    ops: &mut Vec<Op>,
    report: &ComplianceReport,
    fonts: &Fonts,
    y: &mut f32,
) {
    for req in &report.requirements {
        *y = maybe_paginate(pages, ops, *y);

        emit_text(
            ops,
            format!("{} - {}", req.id, req.status),
            FONT_SIZE_SUBHEADING,
            MARGIN + 5.0,
            *y,
            &fonts.bold,
        );
        *y -= LINE_HEIGHT;

        emit_text(
            ops,
            &req.description,
            FONT_SIZE_BODY,
            MARGIN + 10.0,
            *y,
            &fonts.regular,
        );
        *y -= LINE_HEIGHT;

        emit_text(
            ops,
            format!("Proven from: {}", req.theorem),
            FONT_SIZE_BODY,
            MARGIN + 10.0,
            *y,
            &fonts.regular,
        );
        *y -= LINE_HEIGHT;

        if let Some(notes) = &req.notes {
            emit_text(
                ops,
                format!("Notes: {notes}"),
                FONT_SIZE_BODY,
                MARGIN + 10.0,
                *y,
                &fonts.regular,
            );
            *y -= LINE_HEIGHT;
        }

        *y -= LINE_HEIGHT * 0.5;
    }
}

/// Render the footer on the final page.
fn render_footer(ops: &mut Vec<Op>, fonts: &Fonts) {
    let mut y = MARGIN;
    emit_text(
        ops,
        "This report was automatically generated from formally verified TLA+ specifications.",
        FONT_SIZE_BODY - 2.0,
        MARGIN,
        y,
        &fonts.regular,
    );
    y -= LINE_HEIGHT;
    emit_text(
        ops,
        "All proofs are mechanically checked and reproducible.",
        FONT_SIZE_BODY - 2.0,
        MARGIN,
        y,
        &fonts.regular,
    );
}

/// Register the builtin Helvetica + Helvetica-Bold parsed fonts on the
/// document so `PdfSaveOptions::subset_fonts` has something to subset and the
/// PDF's `/Resources /Font` dictionary is populated.
///
/// For 0.7 parity this step is effectively the replacement for
/// `doc.add_builtin_font(BuiltinFont::Helvetica)`. The returned `FontId`s are
/// *not* used by `Op::SetFont` (we use `PdfFontHandle::Builtin(..)` there) but
/// they ensure the font resources are registered in the PDF.
fn register_builtin_fonts(doc: &mut PdfDocument) {
    if let Some(regular) = BuiltinFont::Helvetica.get_parsed_font() {
        let _: printpdf::FontId = doc.add_font(&regular);
    }
    if let Some(bold) = BuiltinFont::HelveticaBold.get_parsed_font() {
        let _: printpdf::FontId = doc.add_font(&bold);
    }
    // Silence unused-import style warning if ParsedFont ever goes unreferenced.
    let _ = std::marker::PhantomData::<ParsedFont>;
}

/// Generate a PDF compliance report.
pub fn generate_pdf(report: &ComplianceReport) -> Result<Vec<u8>> {
    let mut doc = PdfDocument::new(&format!("{} Compliance Report", report.framework));
    register_builtin_fonts(&mut doc);

    let fonts = Fonts::builtin();
    let mut pages: Vec<PdfPage> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y_position: f32 = PAGE_HEIGHT_MM - MARGIN;

    render_header(&mut ops, report, &fonts, &mut y_position);
    render_verification_summary(&mut ops, report, &fonts, &mut y_position);
    render_core_properties(&mut ops, report, &fonts, &mut y_position);
    render_framework_metrics(&mut ops, report.framework, &fonts, &mut y_position);

    emit_text(
        &mut ops,
        "Framework Requirements",
        FONT_SIZE_HEADING,
        MARGIN,
        y_position,
        &fonts.bold,
    );
    y_position -= LINE_HEIGHT * 1.5;

    render_requirements(&mut pages, &mut ops, report, &fonts, &mut y_position);

    render_footer(&mut ops, &fonts);

    // Finalise the last page.
    pages.push(PdfPage::new(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), ops));

    let save_opts = PdfSaveOptions::default();
    let mut warnings = Vec::new();
    let bytes = doc.with_pages(pages).save(&save_opts, &mut warnings);

    if bytes.is_empty() {
        return Err(crate::ComplianceError::PdfError(format!(
            "printpdf 0.9 produced an empty document (warnings: {})",
            warnings.len()
        )));
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComplianceFramework, ComplianceReport};

    #[test]
    fn test_pdf_generation() {
        let report = ComplianceReport::generate(ComplianceFramework::HIPAA).unwrap();
        let pdf = generate_pdf(&report).unwrap();
        assert!(!pdf.is_empty());
        // PDF should start with %PDF header
        assert_eq!(&pdf[0..4], b"%PDF");
    }
}
