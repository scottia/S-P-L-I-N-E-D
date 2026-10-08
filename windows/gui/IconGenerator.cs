using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;

internal static class IconGenerator
{
    private static readonly int[] IconSizes = { 16, 24, 32, 48, 64, 128, 256 };

    private static int Main(string[] args)
    {
        if (args.Length < 2 || args.Length > 3) return 2;
        string logoPath = args[0];
        string iconPath = args[1];
        string previewPath = args.Length == 3 ? args[2] : null;
        if (!File.Exists(logoPath)) return 3;

        using (Image source = Image.FromFile(logoPath))
        using (Bitmap mark = ExtractAngularS(source))
        {
            if (!String.IsNullOrWhiteSpace(previewPath))
                mark.Save(previewPath, ImageFormat.Png);
            WriteMultiResolutionIcon(mark, iconPath);
        }
        return 0;
    }

    private static Bitmap ExtractAngularS(Image source)
    {
        // Crop only the large angular S from the existing repository artwork.
        // The full watermark remains a separate, untouched asset.
        Rectangle crop = new Rectangle(
            Math.Max(0, (int)Math.Round(source.Width * 0.035)),
            Math.Max(0, (int)Math.Round(source.Height * 0.040)),
            Math.Min(source.Width, (int)Math.Round(source.Width * 0.930)),
            Math.Min(source.Height, (int)Math.Round(source.Height * 0.680)));
        if (crop.Right > source.Width) crop.Width = source.Width - crop.Left;
        if (crop.Bottom > source.Height) crop.Height = source.Height - crop.Top;

        using (Bitmap cropped = new Bitmap(crop.Width, crop.Height, PixelFormat.Format32bppArgb))
        {
            using (Graphics graphics = Graphics.FromImage(cropped))
            {
                graphics.Clear(Color.Transparent);
                graphics.CompositingMode = CompositingMode.SourceCopy;
                graphics.DrawImage(source, new Rectangle(0, 0, crop.Width, crop.Height), crop, GraphicsUnit.Pixel);
            }
            RemoveLogoCanvas(cropped, crop);
            ApplyAngularSMask(cropped, crop, source.Size);

            Bitmap output = new Bitmap(1024, 1024, PixelFormat.Format32bppArgb);
            using (Graphics graphics = Graphics.FromImage(output))
            {
                graphics.Clear(Color.Transparent);
                graphics.CompositingMode = CompositingMode.SourceOver;
                graphics.CompositingQuality = CompositingQuality.HighQuality;
                graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                graphics.SmoothingMode = SmoothingMode.HighQuality;
                const int padding = 38;
                float scale = Math.Min((1024f - padding * 2f) / cropped.Width, (1024f - padding * 2f) / cropped.Height);
                int width = Math.Max(1, (int)Math.Round(cropped.Width * scale));
                int height = Math.Max(1, (int)Math.Round(cropped.Height * scale));
                Rectangle target = new Rectangle((1024 - width) / 2, (1024 - height) / 2, width, height);
                graphics.DrawImage(cropped, target, 0, 0, cropped.Width, cropped.Height, GraphicsUnit.Pixel);
            }
            return output;
        }
    }

    private static void ApplyAngularSMask(Bitmap bitmap, Rectangle sourceCrop, Size sourceSize)
    {
        using (Bitmap mask = new Bitmap(bitmap.Width, bitmap.Height, PixelFormat.Format32bppArgb))
        {
            using (Graphics graphics = Graphics.FromImage(mask))
            using (SolidBrush brush = new SolidBrush(Color.White))
            {
                graphics.Clear(Color.Transparent);
                graphics.SmoothingMode = SmoothingMode.AntiAlias;
                graphics.FillPolygon(brush, Polygon(sourceCrop, sourceSize, new[]
                {
                    new PointF(.160f, .500f), new PointF(.195f, .375f), new PointF(.285f, .175f),
                    new PointF(.445f, .045f), new PointF(.975f, .045f), new PointF(.865f, .270f),
                    new PointF(.505f, .275f), new PointF(.305f, .450f)
                }));
                graphics.FillPolygon(brush, Polygon(sourceCrop, sourceSize, new[]
                {
                    new PointF(.160f, .500f), new PointF(.295f, .385f), new PointF(.465f, .245f),
                    new PointF(.835f, .400f), new PointF(.815f, .475f), new PointF(.700f, .585f),
                    new PointF(.465f, .585f), new PointF(.230f, .480f)
                }));
                graphics.FillPolygon(brush, Polygon(sourceCrop, sourceSize, new[]
                {
                    new PointF(.035f, .720f), new PointF(.215f, .525f), new PointF(.470f, .505f),
                    new PointF(.840f, .395f), new PointF(.760f, .555f), new PointF(.700f, .710f),
                    new PointF(.600f, .725f), new PointF(.225f, .715f)
                }));
            }

            Rectangle area = new Rectangle(0, 0, bitmap.Width, bitmap.Height);
            BitmapData imageData = bitmap.LockBits(area, ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
            BitmapData maskData = mask.LockBits(area, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            try
            {
                int count = Math.Abs(imageData.Stride) / 4 * imageData.Height;
                int[] pixels = new int[count];
                int[] masks = new int[count];
                Marshal.Copy(imageData.Scan0, pixels, 0, count);
                Marshal.Copy(maskData.Scan0, masks, 0, count);
                for (int index = 0; index < count; index++)
                {
                    int imageAlpha = (pixels[index] >> 24) & 255;
                    int maskAlpha = (masks[index] >> 24) & 255;
                    int alpha = imageAlpha * maskAlpha / 255;
                    pixels[index] = (pixels[index] & 0x00FFFFFF) | (alpha << 24);
                }
                Marshal.Copy(pixels, 0, imageData.Scan0, count);
            }
            finally
            {
                bitmap.UnlockBits(imageData);
                mask.UnlockBits(maskData);
            }
        }
    }

    private static PointF[] Polygon(Rectangle crop, Size sourceSize, PointF[] normalized)
    {
        PointF[] points = new PointF[normalized.Length];
        for (int index = 0; index < normalized.Length; index++)
        {
            points[index] = new PointF(
                normalized[index].X * sourceSize.Width - crop.Left,
                normalized[index].Y * sourceSize.Height - crop.Top);
        }
        return points;
    }

    private static void RemoveLogoCanvas(Bitmap bitmap, Rectangle sourceCrop)
    {
        Rectangle area = new Rectangle(0, 0, bitmap.Width, bitmap.Height);
        BitmapData data = bitmap.LockBits(area, ImageLockMode.ReadWrite, PixelFormat.Format32bppArgb);
        try
        {
            int count = Math.Abs(data.Stride) / 4 * data.Height;
            int[] pixels = new int[count];
            Marshal.Copy(data.Scan0, pixels, 0, count);
            int rowPixels = Math.Abs(data.Stride) / 4;
            for (int y = 0; y < bitmap.Height; y++)
            {
                int originalY = sourceCrop.Top + y;
                for (int x = 0; x < bitmap.Width; x++)
                {
                    int index = y * rowPixels + x;
                    int value = pixels[index];
                    int blue = value & 255;
                    int green = (value >> 8) & 255;
                    int red = (value >> 16) & 255;
                    int maximum = Math.Max(red, Math.Max(green, blue));
                    int minimum = Math.Min(red, Math.Min(green, blue));
                    int chroma = maximum - minimum;
                    int originalX = sourceCrop.Left + x;

                    // Remove the old canvas/glow and nearby frame lines while
                    // retaining the bright multicolored S pixels exactly.
                    bool frame = (originalY >= sourceCrop.Top + 55 && originalY <= sourceCrop.Top + 130 && originalX < 585)
                        || (originalX > sourceCrop.Right - 28 && originalY > sourceCrop.Top + 65);
                    int alpha;
                    if (frame || maximum < 72 || (maximum < 145 && chroma < 34)) alpha = 0;
                    else if (maximum < 150) alpha = Math.Min(180, (maximum - 70) * 2);
                    else alpha = Math.Min(255, 185 + (maximum - 150));
                    pixels[index] = (alpha << 24) | (red << 16) | (green << 8) | blue;
                }
            }
            Marshal.Copy(pixels, 0, data.Scan0, count);
        }
        finally { bitmap.UnlockBits(data); }
    }

    private static void WriteMultiResolutionIcon(Bitmap master, string path)
    {
        List<byte[]> payloads = new List<byte[]>();
        foreach (int size in IconSizes)
        {
            using (Bitmap frame = new Bitmap(size, size, PixelFormat.Format32bppArgb))
            using (Graphics graphics = Graphics.FromImage(frame))
            using (MemoryStream stream = new MemoryStream())
            {
                graphics.Clear(Color.Transparent);
                graphics.CompositingQuality = CompositingQuality.HighQuality;
                graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                graphics.SmoothingMode = SmoothingMode.HighQuality;
                graphics.DrawImage(master, new Rectangle(0, 0, size, size));
                frame.Save(stream, ImageFormat.Png);
                payloads.Add(stream.ToArray());
            }
        }

        using (FileStream output = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.None))
        using (BinaryWriter writer = new BinaryWriter(output))
        {
            writer.Write((ushort)0);
            writer.Write((ushort)1);
            writer.Write((ushort)IconSizes.Length);
            int offset = 6 + IconSizes.Length * 16;
            for (int index = 0; index < IconSizes.Length; index++)
            {
                int size = IconSizes[index];
                writer.Write((byte)(size == 256 ? 0 : size));
                writer.Write((byte)(size == 256 ? 0 : size));
                writer.Write((byte)0);
                writer.Write((byte)0);
                writer.Write((ushort)1);
                writer.Write((ushort)32);
                writer.Write(payloads[index].Length);
                writer.Write(offset);
                offset += payloads[index].Length;
            }
            foreach (byte[] payload in payloads) writer.Write(payload);
        }
    }
}
