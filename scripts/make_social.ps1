Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing.dll -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Drawing.Text;
using System.Runtime.InteropServices;

public static class SocialBanner3 {
  static bool IsBlue(Color c) {
    return c.B > 140 && c.B > c.R + 25 && c.B > c.G + 25;
  }

  static bool IsLightCanvas(Color c) {
    if (IsBlue(c)) return false;
    int max = Math.Max(c.R, Math.Max(c.G, c.B));
    int min = Math.Min(c.R, Math.Min(c.G, c.B));
    return max >= 190 && (max - min) <= 45;
  }

  public static Bitmap KnockoutCanvas(Bitmap src) {
    int w = src.Width, h = src.Height;
    var dst = new Bitmap(w, h, PixelFormat.Format32bppArgb);
    using (var g = Graphics.FromImage(dst)) g.DrawImage(src, 0, 0, w, h);

    var data = dst.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
    int stride = data.Stride;
    byte[] px = new byte[stride * h];
    Marshal.Copy(data.Scan0, px, 0, px.Length);

    Func<int,int,Color> get = (x, y) => {
      int i = y * stride + x * 4;
      return Color.FromArgb(px[i + 3], px[i + 2], px[i + 1], px[i]);
    };
    Action<int,int> clear = (x, y) => {
      int i = y * stride + x * 4;
      px[i] = 0; px[i + 1] = 0; px[i + 2] = 0; px[i + 3] = 0;
    };

    var seen = new bool[w * h];
    var q = new Queue<Point>();
    Action<int,int> enqueue = (x, y) => {
      if (x < 0 || y < 0 || x >= w || y >= h) return;
      int k = y * w + x;
      if (seen[k]) return;
      if (!IsLightCanvas(get(x, y))) return;
      seen[k] = true;
      q.Enqueue(new Point(x, y));
    };
    enqueue(0, 0); enqueue(w - 1, 0); enqueue(0, h - 1); enqueue(w - 1, h - 1);
    enqueue(w / 2, 0); enqueue(w / 2, h - 1); enqueue(0, h / 2); enqueue(w - 1, h / 2);

    while (q.Count > 0) {
      var p = q.Dequeue();
      clear(p.X, p.Y);
      enqueue(p.X + 1, p.Y);
      enqueue(p.X - 1, p.Y);
      enqueue(p.X, p.Y + 1);
      enqueue(p.X, p.Y - 1);
    }

    Marshal.Copy(px, 0, data.Scan0, px.Length);
    dst.UnlockBits(data);
    return dst;
  }

  static GraphicsPath RoundRect(RectangleF r, float radius) {
    var p = new GraphicsPath();
    float d = radius * 2f;
    p.AddArc(r.X, r.Y, d, d, 180, 90);
    p.AddArc(r.Right - d, r.Y, d, d, 270, 90);
    p.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90);
    p.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
    p.CloseFigure();
    return p;
  }

  public static void Write(string logoPath, string outPath) {
    using (var raw = (Bitmap)Image.FromFile(logoPath))
    using (var logo = KnockoutCanvas(raw))
    using (var bmp = new Bitmap(1280, 640, PixelFormat.Format32bppArgb))
    using (var g = Graphics.FromImage(bmp)) {
      g.SmoothingMode = SmoothingMode.AntiAlias;
      g.InterpolationMode = InterpolationMode.HighQualityBicubic;
      g.PixelOffsetMode = PixelOffsetMode.HighQuality;
      g.TextRenderingHint = TextRenderingHint.ClearTypeGridFit;
      g.Clear(Color.FromArgb(255, 13, 17, 23));

      using (var titleFont = new Font("Segoe UI Semibold", 64f, FontStyle.Bold, GraphicsUnit.Pixel))
      using (var subFont = new Font("Segoe UI", 26f, FontStyle.Regular, GraphicsUnit.Pixel))
      using (var titleBrush = new SolidBrush(Color.FromArgb(255, 240, 246, 252)))
      using (var subBrush = new SolidBrush(Color.FromArgb(255, 139, 148, 158))) {
        var title = "Unseat";
        var sub = "Always-on-top Windows sitting timer";
        var ts = g.MeasureString(title, titleFont);
        var ss = g.MeasureString(sub, subFont);

        float icon = 168f;
        float gap = 40f;
        float textW = Math.Max(ts.Width, ss.Width);
        float textH = ts.Height + 8f + ss.Height;
        float blockH = Math.Max(icon, textH);
        float totalW = icon + gap + textW;
        float ox = (1280f - totalW) / 2f;
        float oy = (640f - blockH) / 2f;

        var dest = new RectangleF(ox, oy + (blockH - icon) / 2f, icon, icon);
        using (var clip = RoundRect(dest, 38f)) {
          g.SetClip(clip);
          g.DrawImage(logo, dest);
          g.ResetClip();
        }

        float tx = ox + icon + gap;
        float ty = oy + (blockH - textH) / 2f;
        g.DrawString(title, titleFont, titleBrush, tx, ty);
        g.DrawString(sub, subFont, subBrush, tx, ty + ts.Height + 2f);
      }

      bmp.Save(outPath, ImageFormat.Png);
      logo.Save(System.IO.Path.Combine(System.IO.Path.GetDirectoryName(outPath), "icon.png"), ImageFormat.Png);
    }
  }
}
"@

[SocialBanner3]::Write(
  "D:\Projects\unseat\assets\logo.png",
  "D:\Projects\unseat\assets\social.png"
)
"wrote social.png"
