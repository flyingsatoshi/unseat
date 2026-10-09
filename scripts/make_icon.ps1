# Renders the Unseat icon (progress ring + stand-up arrow) and packs assets/unseat.ico.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/make_icon.ps1
param([string]$Root = (Split-Path -Parent $PSScriptRoot))

Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing.dll -TypeDefinition @"
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;

public static class UnseatIcon {
  static GraphicsPath RoundRect(float x, float y, float w, float h, float r) {
    var p = new GraphicsPath();
    float d = r * 2;
    p.AddArc(x, y, d, d, 180, 90);
    p.AddArc(x + w - d, y, d, d, 270, 90);
    p.AddArc(x + w - d, y + h - d, d, d, 0, 90);
    p.AddArc(x, y + h - d, d, d, 90, 90);
    p.CloseFigure();
    return p;
  }

  static Pen RoundPen(Color c, float width) {
    var pen = new Pen(c, width);
    pen.StartCap = LineCap.Round;
    pen.EndCap = LineCap.Round;
    pen.LineJoin = LineJoin.Round;
    return pen;
  }

  // Draws one size. Small sizes get proportionally heavier strokes and drop the crown.
  static void Draw(Graphics g, float s, int target, bool background) {
    g.SmoothingMode = SmoothingMode.AntiAlias;
    g.PixelOffsetMode = PixelOffsetMode.HighQuality;
    bool tiny = target <= 24;
    bool small = target <= 48;

    float inset = s * (tiny ? 0.0f : 0.035f);
    float side = s - inset * 2;
    if (background) {
      using (var path = RoundRect(inset, inset, side, side, side * 0.235f))
      using (var fill = new LinearGradientBrush(new RectangleF(0, 0, s, s),
          Color.FromArgb(255, 0x47, 0x74, 0xFF), Color.FromArgb(255, 0x22, 0x36, 0xD4), 60f)) {
        g.FillPath(fill, path);
        if (!tiny) {
          using (var gloss = new LinearGradientBrush(new RectangleF(0, 0, s, s),
              Color.White, Color.White, 90f)) {
            var blend = new ColorBlend();
            blend.Positions = new[] { 0f, 0.55f, 1f };
            blend.Colors = new[] {
              Color.FromArgb(30, 255, 255, 255), Color.FromArgb(0, 255, 255, 255),
              Color.FromArgb(0, 255, 255, 255)
            };
            gloss.InterpolationColors = blend;
            g.FillPath(gloss, path);
          }
          using (var edge = new Pen(Color.FromArgb(40, 255, 255, 255), s * 0.006f)) {
            g.DrawPath(edge, path);
          }
        }
      }
    }

    float cx = s * 0.5f;
    float cy = s * (tiny ? 0.5f : 0.535f);
    float ring = s * (tiny ? 0.315f : small ? 0.275f : 0.265f);
    float stroke = s * (tiny ? 0.125f : small ? 0.095f : 0.082f);
    var box = new RectangleF(cx - ring, cy - ring, ring * 2, ring * 2);

    using (var track = new Pen(Color.FromArgb(tiny ? 90 : 72, 255, 255, 255), stroke)) {
      g.DrawEllipse(track, box);
    }
    using (var arc = RoundPen(Color.White, stroke)) {
      g.DrawArc(arc, box, -90, 270);
    }

    if (!tiny) {
      float cw = s * 0.11f, ch = s * 0.05f;
      float top = cy - ring - stroke * 0.5f - s * 0.035f - ch;
      using (var crown = RoundRect(cx - cw / 2, top, cw, ch, ch / 2))
      using (var white = new SolidBrush(Color.White)) {
        g.FillPath(white, crown);
      }
    }

    float aw = s * (tiny ? 0.135f : 0.105f);
    float ah = s * (tiny ? 0.15f : 0.12f);
    float astroke = s * (tiny ? 0.11f : small ? 0.08f : 0.07f);
    float tipY = cy - ah * 0.95f;
    float baseY = cy + ah * 0.95f;
    using (var arrow = RoundPen(Color.White, astroke)) {
      g.DrawLine(arrow, cx, tipY, cx, baseY);
      g.DrawLines(arrow, new[] {
        new PointF(cx - aw, tipY + aw), new PointF(cx, tipY), new PointF(cx + aw, tipY + aw)
      });
    }
  }

  public static Bitmap Render(int size, bool background) {
    int ss = size <= 64 ? 8 : 4;
    using (var big = new Bitmap(size * ss, size * ss, PixelFormat.Format32bppArgb)) {
      using (var g = Graphics.FromImage(big)) {
        g.Clear(Color.Transparent);
        Draw(g, size * ss, size, background);
      }
      var outBmp = new Bitmap(size, size, PixelFormat.Format32bppArgb);
      using (var g = Graphics.FromImage(outBmp)) {
        g.Clear(Color.Transparent);
        g.InterpolationMode = InterpolationMode.HighQualityBicubic;
        g.PixelOffsetMode = PixelOffsetMode.HighQuality;
        g.CompositingMode = CompositingMode.SourceCopy;
        g.DrawImage(big, new Rectangle(0, 0, size, size));
      }
      return outBmp;
    }
  }
}
"@

$assets = Join-Path $Root "assets"
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$pngs = @()
foreach ($size in $sizes) {
  $bmp = [UnseatIcon]::Render($size, $true)
  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  $pngs += , $ms.ToArray()
}

# ICO container with PNG-compressed entries (supported since Windows Vista).
$ico = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ico
$w.Write([UInt16]0); $w.Write([UInt16]1); $w.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $dim = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
  $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([UInt16]1); $w.Write([UInt16]32)
  $w.Write([UInt32]$pngs[$i].Length); $w.Write([UInt32]$offset)
  $offset += $pngs[$i].Length
}
foreach ($png in $pngs) { $w.Write($png) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $assets "unseat.ico"), $ico.ToArray())

foreach ($pair in @(@("icon.png", 512), @("logo.png", 512), @("unseat.png", 1024))) {
  $bmp = [UnseatIcon]::Render($pair[1], $true)
  $bmp.Save((Join-Path $assets $pair[0]), [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}

# Preview sheet of every ICO size on dark and light backgrounds.
$sheet = New-Object System.Drawing.Bitmap 760, 220
$g = [System.Drawing.Graphics]::FromImage($sheet)
$g.Clear([System.Drawing.Color]::FromArgb(255, 32, 32, 34))
$g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 243, 243, 243))), 0, 110, 760, 110)
$x = 12
foreach ($size in $sizes) {
  $bmp = [UnseatIcon]::Render($size, $true)
  $draw = [Math]::Min($size, 96)
  $g.DrawImage($bmp, $x, [int](55 - $draw / 2), $draw, $draw)
  $g.DrawImage($bmp, $x, [int](165 - $draw / 2), $draw, $draw)
  $x += $draw + 14
  $bmp.Dispose()
}
$g.Dispose()
$preview = Join-Path $Root "target\icon-preview.png"
$sheet.Save($preview, [System.Drawing.Imaging.ImageFormat]::Png)
$sheet.Dispose()
"wrote unseat.ico ($($sizes -join ', ')), icon.png, logo.png, unseat.png, $preview"
