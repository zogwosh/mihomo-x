$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$repository = Split-Path -Parent $PSScriptRoot
$images = @()
foreach ($size in @(16, 32, 48, 256)) {
    $bitmap = [Drawing.Bitmap]::new($size, $size)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.TextRenderingHint = [Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $graphics.ScaleTransform($size / 256.0, $size / 256.0)
    $background = [Drawing.SolidBrush]::new([Drawing.ColorTranslator]::FromHtml('#1457d9'))
    $shape = [Drawing.Drawing2D.GraphicsPath]::new()
    $shape.AddArc(0, 0, 104, 104, 180, 90)
    $shape.AddArc(152, 0, 104, 104, 270, 90)
    $shape.AddArc(152, 152, 104, 104, 0, 90)
    $shape.AddArc(0, 152, 104, 104, 90, 90)
    $shape.CloseFigure()
    $graphics.FillPath($background, $shape)
    $font = [Drawing.Font]::new('Segoe UI', 152, [Drawing.FontStyle]::Bold, [Drawing.GraphicsUnit]::Pixel)
    $format = [Drawing.StringFormat]::new()
    $format.Alignment = [Drawing.StringAlignment]::Center
    $format.LineAlignment = [Drawing.StringAlignment]::Center
    $graphics.DrawString('MX', $font, [Drawing.Brushes]::White, [Drawing.RectangleF]::new(0, -5, 256, 256), $format)
    $memory = [IO.MemoryStream]::new()
    $bitmap.Save($memory, [Drawing.Imaging.ImageFormat]::Png)
    $images += [pscustomobject]@{Size = $size; Bytes = $memory.ToArray()}
    $memory.Dispose()
    $format.Dispose()
    $font.Dispose()
    $shape.Dispose()
    $background.Dispose()
    $graphics.Dispose()
    $bitmap.Dispose()
}
$stream = [IO.File]::Create((Join-Path $repository 'assets/mihomo-x.ico'))
$writer = [IO.BinaryWriter]::new($stream)
try {
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$images.Count)
    $offset = 6 + 16 * $images.Count
    foreach ($image in $images) {
        $dimension = $image.Size
        if ($dimension -eq 256) { $dimension = 0 }
        $writer.Write([byte]$dimension)
        $writer.Write([byte]$dimension)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]$image.Bytes.Length)
        $writer.Write([uint32]$offset)
        $offset += $image.Bytes.Length
    }
    foreach ($image in $images) { $writer.Write([byte[]]$image.Bytes) }
} finally {
    $writer.Dispose()
}
