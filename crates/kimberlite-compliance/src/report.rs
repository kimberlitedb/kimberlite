//! PDF report generation for compliance frameworks

use crate::{ComplianceFramework, ComplianceReport, Result};
use printpdf::{BuiltinFont, Mm, PdfDocument, PdfLayerReference};
use std::io::BufWriter;

const FONT_SIZE_TITLE: f32 = 24.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const FONT_SIZE_SUBHEADING: f32 = 12.0;
const FONT_SIZE_BODY: f32 = 10.0;
const MARGIN: f32 = 50.0;
const LINE_HEIGHT: f32 = 14.0;

/// Render the title and framework details section
fn render_header(
    layer: &PdfLayerReference,
    report: &ComplianceReport,
    font_regular: &printpdf::IndirectFontRef,
    font_bold: &printpdf::IndirectFontRef,
    y: &mut f32,
) {
    layer.use_text(
        format!("{} Compliance Report", report.framework),
        FONT_SIZE_TITLE,
        Mm(MARGIN),
        Mm(*y),
        font_bold,
    );
    *y -= LINE_HEIGHT * 2.0;

    layer.use_text(
        format!("Framework: {}", report.framework.full_name()),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(*y),
        font_regular,
    );
    *y -= LINE_HEIGHT;

    layer.use_text(
        format!(
            "Generated: {}",
            report.generated_at.format("%Y-%m-%d %H:%M:%S UTC")
        ),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(*y),
        font_regular,
    );
    *y -= LINE_HEIGHT * 2.0;
}

/// Render the verification summary section
fn render_verification_summary(
    layer: &PdfLayerReference,
    report: &ComplianceReport,
    font_regular: &printpdf::IndirectFontRef,
    font_bold: &printpdf::IndirectFontRef,
    y: &mut f32,
) {
    layer.use_text(
        "Verification Summary",
        FONT_SIZE_HEADING,
        Mm(MARGIN),
        Mm(*y),
        font_bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    layer.use_text(
        format!(
            "Status: {} of {} requirements verified ({:.1}%)",
            report.certificate.verified_count,
            report.certificate.total_requirements,
            report.certificate.verification_percentage()
        ),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(*y),
        font_regular,
    );
    *y -= LINE_HEIGHT;

    layer.use_text(
        format!("Toolchain: {}", report.certificate.toolchain_version),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(*y),
        font_regular,
    );
    *y -= LINE_HEIGHT;

    layer.use_text(
        format!("Specification Hash: {}", report.certificate.spec_hash),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(*y),
        font_regular,
    );
    *y -= LINE_HEIGHT * 2.0;
}

/// Render the core compliance properties section
fn render_core_properties(
    layer: &PdfLayerReference,
    report: &ComplianceReport,
    font_regular: &printpdf::IndirectFontRef,
    font_bold: &printpdf::IndirectFontRef,
    y: &mut f32,
) {
    layer.use_text(
        "Core Compliance Properties",
        FONT_SIZE_HEADING,
        Mm(MARGIN),
        Mm(*y),
        font_bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    for (property, status) in &report.core_properties {
        let status_str = if *status {
            "✓ Satisfied"
        } else {
            "✗ Not satisfied"
        };
        layer.use_text(
            format!("{property}: {status_str}"),
            FONT_SIZE_BODY,
            Mm(MARGIN + 10.0),
            Mm(*y),
            font_regular,
        );
        *y -= LINE_HEIGHT;
    }
    *y -= LINE_HEIGHT;
}

/// Render SLA metrics section (SOC 2 reports)
fn render_sla_metrics(
    layer: &PdfLayerReference,
    font_regular: &printpdf::IndirectFontRef,
    font_bold: &printpdf::IndirectFontRef,
    y: &mut f32,
) {
    layer.use_text(
        "SLA Metrics (SOC 2 A1.2 / CC7.4)",
        FONT_SIZE_HEADING,
        Mm(MARGIN),
        Mm(*y),
        font_bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    let metrics = [
        ("Availability Target", "99.99% uptime SLA"),
        ("Recovery Point Objective (RPO)", "0 (append-only log, no data loss)"),
        ("Recovery Time Objective (RTO)", "< 30s (state reconstruction from log)"),
        ("Backup Verification", "Hash chain integrity check on every restore"),
        ("Incident Response Time", "72h notification deadline (automated tracking)"),
    ];

    for (metric, value) in &metrics {
        layer.use_text(
            format!("{metric}: {value}"),
            FONT_SIZE_BODY,
            Mm(MARGIN + 10.0),
            Mm(*y),
            font_regular,
        );
        *y -= LINE_HEIGHT;
    }
    *y -= LINE_HEIGHT;
}

/// Render security metrics section (ISO 27001 reports)
fn render_security_metrics(
    layer: &PdfLayerReference,
    font_regular: &printpdf::IndirectFontRef,
    font_bold: &printpdf::IndirectFontRef,
    y: &mut f32,
) {
    layer.use_text(
        "Security Metrics Summary (ISO 27001 A.12.4)",
        FONT_SIZE_HEADING,
        Mm(MARGIN),
        Mm(*y),
        font_bold,
    );
    *y -= LINE_HEIGHT * 1.5;

    let metrics = [
        ("Encryption Algorithm", "AES-256-GCM (FIPS 140-2 validated)"),
        ("Hash Chain Algorithm", "SHA-256 (compliance) / BLAKE3 (internal)"),
        ("Signature Algorithm", "Ed25519 (per-tenant and per-record)"),
        ("Access Control Model", "RBAC (4 roles) + ABAC (19 policies)"),
        ("Audit Log Integrity", "Immutable append-only with hash chain verification"),
        ("Tenant Isolation", "Per-tenant encryption keys, placement routing"),
    ];

    for (metric, value) in &metrics {
        layer.use_text(
            format!("{metric}: {value}"),
            FONT_SIZE_BODY,
            Mm(MARGIN + 10.0),
            Mm(*y),
            font_regular,
        );
        *y -= LINE_HEIGHT;
    }
    *y -= LINE_HEIGHT;
}

/// Render framework-specific metrics (SOC 2 SLA, ISO 27001 security)
fn render_framework_metrics(
    layer: &PdfLayerReference,
    framework: ComplianceFramework,
    font_regular: &printpdf::IndirectFontRef,
    font_bold: &printpdf::IndirectFontRef,
    y: &mut f32,
) {
    match framework {
        ComplianceFramework::SOC2 => render_sla_metrics(layer, font_regular, font_bold, y),
        ComplianceFramework::ISO27001 => {
            render_security_metrics(layer, font_regular, font_bold, y);
        }
        _ => {}
    }
}

/// Render the individual requirement entries with multi-page support
fn render_requirements(
    doc: &printpdf::PdfDocumentReference,
    report: &ComplianceReport,
    font_regular: &printpdf::IndirectFontRef,
    font_bold: &printpdf::IndirectFontRef,
    page: printpdf::PdfPageIndex,
    layer_idx: printpdf::PdfLayerIndex,
    y: &mut f32,
) -> (printpdf::PdfPageIndex, printpdf::PdfLayerIndex) {
    let mut current_page = page;
    let mut current_layer_idx = layer_idx;

    for req in &report.requirements {
        if *y < MARGIN + 50.0 {
            let (new_page, new_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
            current_page = new_page;
            current_layer_idx = new_layer;
            *y = 297.0 - MARGIN;
        }

        let layer = doc.get_page(current_page).get_layer(current_layer_idx);

        layer.use_text(
            format!("{} - {}", req.id, req.status),
            FONT_SIZE_SUBHEADING,
            Mm(MARGIN + 5.0),
            Mm(*y),
            font_bold,
        );
        *y -= LINE_HEIGHT;

        layer.use_text(&req.description, FONT_SIZE_BODY, Mm(MARGIN + 10.0), Mm(*y), font_regular);
        *y -= LINE_HEIGHT;

        layer.use_text(
            format!("Proven from: {}", req.theorem),
            FONT_SIZE_BODY,
            Mm(MARGIN + 10.0),
            Mm(*y),
            font_regular,
        );
        *y -= LINE_HEIGHT;

        if let Some(notes) = &req.notes {
            layer.use_text(format!("Notes: {notes}"), FONT_SIZE_BODY, Mm(MARGIN + 10.0), Mm(*y), font_regular);
            *y -= LINE_HEIGHT;
        }

        *y -= LINE_HEIGHT * 0.5;
    }

    (current_page, current_layer_idx)
}

/// Render the footer on the final page
fn render_footer(
    layer: &PdfLayerReference,
    font_regular: &printpdf::IndirectFontRef,
) {
    let mut y = MARGIN;
    layer.use_text(
        "This report was automatically generated from formally verified TLA+ specifications.",
        FONT_SIZE_BODY - 2.0,
        Mm(MARGIN),
        Mm(y),
        font_regular,
    );
    y -= LINE_HEIGHT;
    layer.use_text(
        "All proofs are mechanically checked and reproducible.",
        FONT_SIZE_BODY - 2.0,
        Mm(MARGIN),
        Mm(y),
        font_regular,
    );
}

/// Generate a PDF compliance report
pub fn generate_pdf(report: &ComplianceReport) -> Result<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new(
        format!("{} Compliance Report", report.framework),
        Mm(210.0),
        Mm(297.0),
        "Layer 1",
    );

    let font_regular = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;

    let current_layer = doc.get_page(page1).get_layer(layer1);
    let mut y_position = 297.0 - MARGIN;

    render_header(&current_layer, report, &font_regular, &font_bold, &mut y_position);
    render_verification_summary(&current_layer, report, &font_regular, &font_bold, &mut y_position);
    render_core_properties(&current_layer, report, &font_regular, &font_bold, &mut y_position);
    render_framework_metrics(&current_layer, report.framework, &font_regular, &font_bold, &mut y_position);

    current_layer.use_text("Framework Requirements", FONT_SIZE_HEADING, Mm(MARGIN), Mm(y_position), &font_bold);
    y_position -= LINE_HEIGHT * 1.5;

    let (final_page, final_layer_idx) =
        render_requirements(&doc, report, &font_regular, &font_bold, page1, layer1, &mut y_position);

    let final_layer = doc.get_page(final_page).get_layer(final_layer_idx);
    render_footer(&final_layer, &font_regular);

    let mut buf = BufWriter::new(Vec::new());
    doc.save(&mut buf)?;
    buf.into_inner().map_err(|e| {
        crate::ComplianceError::ReportGeneration(format!("Failed to finalize PDF: {e}"))
    })
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
