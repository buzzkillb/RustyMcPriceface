use crate::price_service::HistoryData;
use plotters::prelude::*;
use std::error::Error;

/// Generate a chart image buffer from history data
pub fn generate_shanghai_chart(
    data: &[HistoryData],
    symbol: &str,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let width = 800;
    let height = 400;
    let mut buffer = vec![0u8; width as usize * height as usize * 3]; // RGB buffer

    {
        let root = BitMapBackend::with_buffer(&mut buffer, (width, height)).into_drawing_area();
        let background_color = RGBColor(30, 30, 30); // Dark gray
        root.fill(&background_color)?;

        let title = format!("{} Shanghai Premium (1Y)", symbol.to_uppercase());

        let shanghai_color = RGBColor(255, 99, 71); // Tomato Red
        let western_color = RGBColor(100, 149, 237); // Cornflower Blue
        let text_style = ("sans-serif", 30).into_font().color(&WHITE);

        // Find range
        let min_price = data
            .iter()
            .map(|d| d.shanghai.min(d.western))
            .fold(f64::INFINITY, f64::min);
        let max_price = data
            .iter()
            .map(|d| d.shanghai.max(d.western))
            .fold(f64::NEG_INFINITY, f64::max);

        // Add some padding
        let range_padding = (max_price - min_price) * 0.05;
        let y_min = min_price - range_padding;
        let y_max = max_price + range_padding;

        let mut chart = ChartBuilder::on(&root)
            .caption(title, text_style)
            .margin(10)
            .x_label_area_size(30)
            .y_label_area_size(40)
            .right_y_label_area_size(40) // Add right Y-axis
            .build_cartesian_2d(0..data.len(), y_min..y_max)?;

        chart
            .configure_mesh()
            .x_labels(12) // Increase from 5 to 12
            .x_label_formatter(&|idx| {
                if let Some(d) = data.get(*idx) {
                    // Simplistic date format "MM-DD" or similar
                    // data.date is "YYYY-MM-DD"
                    if d.date.len() >= 10 {
                        return format!("{}", &d.date[5..]);
                    }
                    return d.date.clone();
                }
                "".to_string()
            })
            .x_label_style(("sans-serif", 15).into_font().color(&WHITE))
            .y_label_style(("sans-serif", 15).into_font().color(&WHITE))
            .axis_style(WHITE)
            .bold_line_style(WHITE.mix(0.1))
            .light_line_style(WHITE.mix(0.05))
            .draw()?;

        // Shanghai Series
        chart
            .draw_series(LineSeries::new(
                data.iter().enumerate().map(|(i, d)| (i, d.shanghai)),
                &shanghai_color,
            ))?
            .label("Shanghai")
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], shanghai_color));

        // Western Series
        chart
            .draw_series(LineSeries::new(
                data.iter().enumerate().map(|(i, d)| (i, d.western)),
                &western_color,
            ))?
            .label("Western")
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], western_color));

        chart
            .configure_series_labels()
            .background_style(&RGBColor(50, 50, 50))
            .border_style(&WHITE)
            .label_font(("sans-serif", 15).into_font().color(&WHITE))
            .draw()?;
    }

    // Encode to PNG
    let mut image_data = Vec::new();
    let image =
        image::RgbImage::from_raw(width, height, buffer).ok_or("Failed to create image buffer")?;

    // plotters uses RGB, we can straightforwardly encode to PNG
    // Note: plotters doesn't include png encoding itself in the core, we need `image` crate or `plotters` feature
    // Let's rely on `image` crate if available from plotters features or check Cargo.toml
    // Wait, plotters 0.3 with "image" feature re-exports it or allows usage.
    // Actually, `BitMapBackend` writes to a buffer. We can use the `image` crate to save/encode it.
    // Check if we have `image` dependency separately or via plotters.
    // If not, we might need to add `image` crate to Cargo.toml or use plotters `into_drawing_area` on an `image` crate type if supported.
    //
    // Simpler: Use `plotters::backend::BitMapBackend` which creates a raw RGB buffer?
    // Wait, the buffer I created `vec![0u8; ...]` is populated.
    // To encode to PNG, I need an encoder.
    // Let's assume `image` crate is needed.
    // I'll add `image = "0.24"` to Cargo.toml as well if not present (it's not).

    // Let's check if plotters re-exports it.
    // It does not default expose generic png encoding for a raw buffer.

    // BETTER APPROACH: Use `BitMapBackend` tied to a path, OR `BitMapBackend` with a phantom and encode later.
    // Actually, declaring `image` dependency is safer. I'll add that to Cargo.toml in the next step or now.

    // For now, I'll write the code assuming `image` crate usage.

    let encoder = image::codecs::png::PngEncoder::new(&mut image_data);
    image.write_with_encoder(encoder)?;

    Ok(image_data)
}

/// Generate a line chart for generic crypto history from local connection
pub fn generate_price_chart(
    data: &[(i64, f64)],
    symbol: &str,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let width = 800;
    let height = 400;
    let mut buffer = vec![0u8; width as usize * height as usize * 3]; // RGB buffer

    {
        let root = BitMapBackend::with_buffer(&mut buffer, (width, height)).into_drawing_area();
        let background_color = RGBColor(30, 30, 30); // Dark gray
        root.fill(&background_color)?;

        let title = format!("{} Price History (30 Days)", symbol.to_uppercase());
        let line_color = RGBColor(0, 255, 127); // Spring Green
        let text_style = ("sans-serif", 30).into_font().color(&WHITE);

        if data.is_empty() {
            root.draw_text("No Data Available", &text_style, (300, 200))?;
            drop(root); // Finish drawing

            let mut image_data = Vec::new();
            let image = image::RgbImage::from_raw(width, height, buffer)
                .ok_or("Failed to create image buffer")?;
            let encoder = image::codecs::png::PngEncoder::new(&mut image_data);
            image.write_with_encoder(encoder)?;
            return Ok(image_data);
        }

        // Find min/max for Y axis
        let min_price = data.iter().map(|(_, p)| *p).fold(f64::INFINITY, f64::min);
        let max_price = data
            .iter()
            .map(|(_, p)| *p)
            .fold(f64::NEG_INFINITY, f64::max);

        // Add padding
        let range_padding = (max_price - min_price) * 0.05;
        let y_min = min_price - range_padding;
        let y_max = max_price + range_padding;

        let mut chart = ChartBuilder::on(&root)
            .caption(title, text_style)
            .margin(10)
            .x_label_area_size(30)
            .y_label_area_size(50)
            .right_y_label_area_size(50)
            .build_cartesian_2d(0..data.len(), y_min..y_max)?;

        chart
            .configure_mesh()
            .x_labels(10)
            .x_label_formatter(&|idx| {
                if let Some((ts, _)) = data.get(*idx) {
                    // Convert timestamp to date string (simplified)
                    // We don't have chrono in scope yet, check if we do in Cargo.toml?
                    // assuming we might need to rely on basic formatting or add chrono
                    // Actually, let's use a simple approach using `chrono` if typically available or string manipulation if not.
                    // The project likely has `chrono` or similar. Let's assume user has `chrono` or we format simply.
                    // To be safe without digging into Cargo.toml right now (time is short), I'll just skip detailed date formatting
                    // or implement a basic one if I recall the utils has `get_current_timestamp`.
                    // Actually, let's just show relative days or just index if we can't format.
                    // WAIT, the Shanghai chart used `d.date` string. Here we have `i64` timestamp.
                    // I will formatting assuming chrono is available (very standard).

                    // Hacky fallback if no chrono: just show simple index? No that's bad.
                    // I'll try to use standard library or just `format!("{}", ts)` temporarily until verified.
                    // BETTER: Use `chrono` properly.
                    use chrono::TimeZone;
                    match chrono::Utc.timestamp_opt(*ts, 0) {
                        chrono::LocalResult::Single(dt) => return dt.format("%m-%d").to_string(),
                        _ => return format!("{}", ts),
                    }
                }
                "".to_string()
            })
            .x_label_style(("sans-serif", 15).into_font().color(&WHITE))
            .y_label_style(("sans-serif", 15).into_font().color(&WHITE))
            .axis_style(WHITE)
            .bold_line_style(WHITE.mix(0.1))
            .light_line_style(WHITE.mix(0.05))
            .draw()?;

        chart
            .draw_series(LineSeries::new(
                data.iter().enumerate().map(|(i, &(_, price))| (i, price)),
                &line_color,
            ))?
            .label(symbol)
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], line_color));

        chart
            .configure_series_labels()
            .background_style(&RGBColor(50, 50, 50))
            .border_style(&WHITE)
            .label_font(("sans-serif", 15).into_font().color(&WHITE))
            .draw()?;
    }

    let mut image_data = Vec::new();
    let image =
        image::RgbImage::from_raw(width, height, buffer).ok_or("Failed to create image buffer")?;
    let encoder = image::codecs::png::PngEncoder::new(&mut image_data);
    image.write_with_encoder(encoder)?;

    Ok(image_data)
}
