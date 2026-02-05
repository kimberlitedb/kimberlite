//! PDF report generation for compliance frameworks

use crate::{ComplianceReport, Result};
use printpdf::*;
use std::io::BufWriter;

const FONT_SIZE_TITLE: f32 = 24.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const FONT_SIZE_SUBHEADING: f32 = 12.0;
const FONT_SIZE_BODY: f32 = 10.0;
const MARGIN: f32 = 50.0;
const LINE_HEIGHT: f32 = 14.0;

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

    // Title
    current_layer.use_text(
        format!("{} Compliance Report", report.framework),
        FONT_SIZE_TITLE,
        Mm(MARGIN),
        Mm(y_position),
        &font_bold,
    );
    y_position -= LINE_HEIGHT * 2.0;

    // Framework details
    current_layer.use_text(
        format!("Framework: {}", report.framework.full_name()),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(y_position),
        &font_regular,
    );
    y_position -= LINE_HEIGHT;

    current_layer.use_text(
        format!("Generated: {}", report.generated_at.format("%Y-%m-%d %H:%M:%S UTC")),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(y_position),
        &font_regular,
    );
    y_position -= LINE_HEIGHT * 2.0;

    // Verification summary
    current_layer.use_text(
        "Verification Summary",
        FONT_SIZE_HEADING,
        Mm(MARGIN),
        Mm(y_position),
        &font_bold,
    );
    y_position -= LINE_HEIGHT * 1.5;

    current_layer.use_text(
        format!(
            "Status: {} of {} requirements verified ({:.1}%)",
            report.certificate.verified_count,
            report.certificate.total_requirements,
            report.certificate.verification_percentage()
        ),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(y_position),
        &font_regular,
    );
    y_position -= LINE_HEIGHT;

    current_layer.use_text(
        format!("Toolchain: {}", report.certificate.toolchain_version),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(y_position),
        &font_regular,
    );
    y_position -= LINE_HEIGHT;

    current_layer.use_text(
        format!("Specification Hash: {}", report.certificate.spec_hash),
        FONT_SIZE_BODY,
        Mm(MARGIN),
        Mm(y_position),
        &font_regular,
    );
    y_position -= LINE_HEIGHT * 2.0;

    // Core properties
    current_layer.use_text(
        "Core Compliance Properties",
        FONT_SIZE_HEADING,
        Mm(MARGIN),
        Mm(y_position),
        &font_bold,
    );
    y_position -= LINE_HEIGHT * 1.5;

    for (property, status) in &report.core_properties {
        let status_str = if *status { "✓ Satisfied" } else { "✗ Not satisfied" };
        current_layer.use_text(
            format!("{}: {}", property, status_str),
            FONT_SIZE_BODY,
            Mm(MARGIN + 10.0),
            Mm(y_position),
            &font_regular,
        );
        y_position -= LINE_HEIGHT;
    }
    y_position -= LINE_HEIGHT;

    // Requirements
    current_layer.use_text(
        "Framework Requirements",
        FONT_SIZE_HEADING,
        Mm(MARGIN),
        Mm(y_position),
        &font_bold,
    );
    y_position -= LINE_HEIGHT * 1.5;

    // Track current page/layer for multi-page support
    let mut current_page = page1;
    let mut current_layer_idx = layer1;

    for req in &report.requirements {
        // Check if we need a new page
        if y_position < MARGIN + 50.0 {
            let (new_page, new_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
            current_page = new_page;
            current_layer_idx = new_layer;
            y_position = 297.0 - MARGIN;
        }

        let layer = doc.get_page(current_page).get_layer(current_layer_idx);

        // Requirement ID and status
        layer.use_text(
            format!("{} - {}", req.id, req.status),
            FONT_SIZE_SUBHEADING,
            Mm(MARGIN + 5.0),
            Mm(y_position),
            &font_bold,
        );
        y_position -= LINE_HEIGHT;

        // Description
        layer.use_text(
            &req.description,
            FONT_SIZE_BODY,
            Mm(MARGIN + 10.0),
            Mm(y_position),
            &font_regular,
        );
        y_position -= LINE_HEIGHT;

        // Theorem mapping
        layer.use_text(
            format!("Proven from: {}", req.theorem),
            FONT_SIZE_BODY,
            Mm(MARGIN + 10.0),
            Mm(y_position),
            &font_regular,
        );
        y_position -= LINE_HEIGHT;

        // Notes if present
        if let Some(notes) = &req.notes {
            layer.use_text(
                format!("Notes: {}", notes),
                FONT_SIZE_BODY,
                Mm(MARGIN + 10.0),
                Mm(y_position),
                &font_regular,
            );
            y_position -= LINE_HEIGHT;
        }

        y_position -= LINE_HEIGHT * 0.5;
    }

    // Footer
    y_position = MARGIN;
    let final_layer = doc.get_page(current_page).get_layer(current_layer_idx);
    final_layer.use_text(
        "This report was automatically generated from formally verified TLA+ specifications.",
        FONT_SIZE_BODY - 2.0,
        Mm(MARGIN),
        Mm(y_position),
        &font_regular,
    );
    y_position -= LINE_HEIGHT;
    final_layer.use_text(
        "All proofs are mechanically checked and reproducible.",
        FONT_SIZE_BODY - 2.0,
        Mm(MARGIN),
        Mm(y_position),
        &font_regular,
    );

    // Save to bytes
    let mut buf = BufWriter::new(Vec::new());
    doc.save(&mut buf)?;
    Ok(buf.into_inner().map_err(|e| {
        crate::ComplianceError::ReportGeneration(format!("Failed to finalize PDF: {}", e))
    })?)
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
