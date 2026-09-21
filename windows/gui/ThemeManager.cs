using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using Microsoft.Win32;

namespace Splined.WindowsGui
{
    internal enum ThemeFontRole { Minor, Body, Control, PanelTitle, AppTitle }
    internal enum ThemeStatusColor { White, Orange, Red, Purple, Green, Blue }
    internal enum CardVisualRole { Panel, Nested, Recommended, Log }

    /// <summary>Central semantic palette. Status colors remain data colors, not decoration.</summary>
    internal sealed class ThemePalette
    {
        public readonly bool Dark;
        public readonly Color WindowBackground, TitlebarBackground, TopNavigationSurface;
        public readonly Color SurfacePrimary, SurfaceSecondary, SurfaceRaised, PanelSurface, NestedCardSurface, TreeSurface, SurfaceHover, SurfacePressed, Shadow;
        public readonly Color BorderSubtle, BorderFocus, TextPrimary, TextSecondary, TextDisabled;
        public readonly Color ButtonBackground, ButtonHover, ButtonPressed, ButtonActive, ButtonActiveHover, ButtonActivePressed, ButtonDisabled;
        public readonly Color AccentPrimary, AccentHover, AccentPressed, PrimaryAction, PrimaryActionHover, PrimaryActionPressed, PrimaryActionForeground, InfoBackground, InfoForeground;
        public readonly Color InputBackground, InputFocusBackground, InputBorder, InputFocusBorder;
        public readonly Color CheckOnBackground, CheckOffBackground, CheckGlyph;
        public readonly Color ScrollbarTrack, ScrollbarThumb, ScrollbarHover;
        public readonly Color Warning, Error, Success;
        public readonly Color LogReadBackground, LogWriteBackground, LogBorder, LogForeground, LogHeading, LogAccent, LogSuccess, LogWarning, LogError, LogMuted;
        public readonly Color Link, LinkActive;
        public readonly Color StatusWhite, StatusOrange, StatusRed, StatusPurple, StatusGreen, StatusBlue;
        public readonly float WatermarkOpacity;

        private ThemePalette(bool dark)
        {
            Dark = dark;
            if (dark)
            {
                // Dark reference: deep navy base with progressively brighter blue/slate surfaces.
                WindowBackground = Color.FromArgb(5, 16, 30);
                TitlebarBackground = Color.FromArgb(6, 22, 39);
                TopNavigationSurface = Color.FromArgb(8, 27, 47);
                SurfacePrimary = Color.FromArgb(7, 23, 40);
                SurfaceSecondary = Color.FromArgb(10, 30, 50);
                SurfaceRaised = Color.FromArgb(12, 33, 54);
                PanelSurface = Color.FromArgb(11, 31, 51);
                NestedCardSurface = Color.FromArgb(14, 37, 60);
                TreeSurface = Color.FromArgb(7, 25, 44);
                SurfaceHover = Color.FromArgb(18, 47, 74);
                SurfacePressed = Color.FromArgb(23, 58, 90);
                Shadow = Color.FromArgb(2, 8, 17);
                BorderSubtle = Color.FromArgb(41, 70, 95);
                BorderFocus = Color.FromArgb(55, 177, 241);
                TextPrimary = Color.FromArgb(244, 248, 252);
                TextSecondary = Color.FromArgb(174, 195, 216);
                TextDisabled = Color.FromArgb(102, 126, 151);
                ButtonBackground = Color.FromArgb(16, 40, 64);
                ButtonHover = Color.FromArgb(22, 52, 81);
                ButtonPressed = Color.FromArgb(28, 64, 98);
                ButtonActive = Color.FromArgb(12, 57, 70);
                ButtonActiveHover = Color.FromArgb(15, 70, 86);
                ButtonActivePressed = Color.FromArgb(18, 82, 99);
                ButtonDisabled = Color.FromArgb(13, 30, 48);
                AccentPrimary = Color.FromArgb(22, 143, 229);
                AccentHover = Color.FromArgb(35, 164, 242);
                AccentPressed = Color.FromArgb(16, 116, 191);
                PrimaryAction = Color.FromArgb(22, 143, 229);
                PrimaryActionHover = Color.FromArgb(35, 164, 242);
                PrimaryActionPressed = Color.FromArgb(16, 116, 191);
                PrimaryActionForeground = Color.White;
                InfoBackground = Color.FromArgb(26, 113, 183);
                InfoForeground = Color.White;
                InputBackground = Color.FromArgb(7, 25, 44);
                InputFocusBackground = Color.FromArgb(10, 35, 58);
                InputBorder = Color.FromArgb(31, 69, 99);
                InputFocusBorder = BorderFocus;
                CheckOnBackground = Color.FromArgb(20, 117, 69);
                CheckOffBackground = Color.FromArgb(139, 39, 50);
                CheckGlyph = Color.FromArgb(255, 216, 74);
                ScrollbarTrack = Color.FromArgb(7, 25, 44);
                ScrollbarThumb = Color.FromArgb(92, 115, 139);
                ScrollbarHover = Color.FromArgb(125, 151, 178);
                Warning = Color.FromArgb(244, 183, 67);
                Error = Color.FromArgb(239, 100, 111);
                Success = Color.FromArgb(62, 216, 129);
                // Scan Activity shares the same navy surface family as Artwork
                // Candidates in every mode. READ/WRITE meaning remains in the
                // banner and text palette, not a red-brown background wash.
                LogReadBackground = PanelSurface;
                LogWriteBackground = PanelSurface;
                LogBorder = BorderSubtle;
                LogForeground = Color.FromArgb(237, 242, 247);
                LogHeading = Color.FromArgb(250, 250, 250);
                LogAccent = Color.FromArgb(89, 201, 239);
                LogSuccess = Color.FromArgb(78, 222, 137);
                LogWarning = Color.FromArgb(247, 184, 67);
                LogError = Color.FromArgb(255, 125, 125);
                LogMuted = Color.FromArgb(181, 196, 211);
                Link = Color.FromArgb(74, 189, 235);
                LinkActive = Color.FromArgb(128, 220, 250);
                StatusWhite = Color.FromArgb(235, 237, 240);
                StatusOrange = Color.FromArgb(235, 166, 92);
                StatusRed = Color.FromArgb(255, 105, 115);
                StatusPurple = Color.FromArgb(188, 145, 235);
                StatusGreen = Color.FromArgb(111, 214, 143);
                StatusBlue = Color.FromArgb(103, 181, 255);
                WatermarkOpacity = 0.13f;
            }
            else
            {
                WindowBackground = Color.FromArgb(243, 244, 246);
                TitlebarBackground = Color.FromArgb(248, 249, 251);
                TopNavigationSurface = Color.FromArgb(247, 249, 252);
                SurfacePrimary = Color.FromArgb(250, 250, 251);
                SurfaceSecondary = Color.FromArgb(246, 247, 249);
                SurfaceRaised = Color.White;
                PanelSurface = Color.White;
                NestedCardSurface = Color.FromArgb(248, 250, 253);
                TreeSurface = Color.White;
                SurfaceHover = Color.FromArgb(240, 241, 243);
                SurfacePressed = Color.FromArgb(231, 234, 238);
                Shadow = Color.FromArgb(190, 197, 207);
                BorderSubtle = Color.FromArgb(216, 220, 226);
                BorderFocus = Color.FromArgb(0, 103, 184);
                TextPrimary = Color.FromArgb(23, 25, 29);
                TextSecondary = Color.FromArgb(95, 102, 112);
                TextDisabled = Color.FromArgb(154, 159, 167);
                ButtonBackground = Color.FromArgb(251, 251, 252);
                ButtonHover = Color.FromArgb(240, 242, 245);
                ButtonPressed = Color.FromArgb(229, 232, 236);
                ButtonActive = Color.FromArgb(231, 243, 252);
                ButtonActiveHover = Color.FromArgb(218, 237, 250);
                ButtonActivePressed = Color.FromArgb(204, 229, 247);
                ButtonDisabled = Color.FromArgb(239, 241, 244);
                AccentPrimary = Color.FromArgb(0, 103, 184);
                AccentHover = Color.FromArgb(0, 120, 212);
                AccentPressed = Color.FromArgb(0, 84, 150);
                PrimaryAction = Color.FromArgb(0, 103, 184);
                PrimaryActionHover = Color.FromArgb(0, 120, 212);
                PrimaryActionPressed = Color.FromArgb(0, 84, 150);
                PrimaryActionForeground = Color.White;
                InfoBackground = Color.FromArgb(37, 119, 178);
                InfoForeground = Color.White;
                InputBackground = Color.White;
                InputFocusBackground = Color.FromArgb(249, 252, 255);
                InputBorder = BorderSubtle;
                InputFocusBorder = BorderFocus;
                CheckOnBackground = Color.FromArgb(27, 126, 73);
                CheckOffBackground = Color.FromArgb(166, 45, 57);
                CheckGlyph = Color.FromArgb(255, 205, 42);
                ScrollbarTrack = SurfacePrimary;
                ScrollbarThumb = Color.FromArgb(173, 179, 187);
                ScrollbarHover = Color.FromArgb(133, 141, 151);
                Warning = Color.FromArgb(154, 82, 0);
                Error = Color.FromArgb(176, 22, 22);
                Success = Color.FromArgb(18, 112, 60);
                LogReadBackground = PanelSurface;
                LogWriteBackground = PanelSurface;
                LogBorder = BorderSubtle;
                LogForeground = Color.FromArgb(28, 31, 34);
                LogHeading = Color.FromArgb(22, 35, 45);
                LogAccent = Color.FromArgb(0, 92, 138);
                LogSuccess = Color.FromArgb(18, 112, 60);
                LogWarning = Color.FromArgb(154, 82, 0);
                LogError = Color.FromArgb(176, 22, 22);
                LogMuted = Color.FromArgb(76, 83, 91);
                Link = Color.FromArgb(0, 103, 184);
                LinkActive = Color.FromArgb(0, 78, 140);
                StatusWhite = Color.FromArgb(80, 86, 94);
                StatusOrange = Color.DarkOrange;
                StatusRed = Color.Firebrick;
                StatusPurple = Color.Purple;
                StatusGreen = Color.ForestGreen;
                StatusBlue = Color.RoyalBlue;
                WatermarkOpacity = 0.060f;
            }
        }

        public static ThemePalette Create(bool dark) { return new ThemePalette(dark); }
    }

    internal static class ThemeManager
    {
        public const int Space4 = 4, Space8 = 8, Space12 = 12, Space16 = 16;
        public const int PanelRadius = 8, CardRadius = 6, ControlRadius = 5;

        private sealed class RoundedBinding { public int Radius; }
        private sealed class InputBinding { public ThemePalette Palette; }
        private sealed class WindowBinding { public string Theme; }
        private sealed class ButtonBinding { public string Theme; public ThemePalette Palette; public bool Hot; public bool Pressed; }
        private sealed class ComboBinding { public ThemePalette Palette; }
        private sealed class CheckBinding { public ThemePalette Palette; }
        private sealed class TreeCheckBinding { public ImageList Images; public Color Off; public Color On; public Color Glyph; public Color Background; public int Size; }
        private sealed class NativeControlBinding { public bool Dark; }

        private static readonly object WatermarkLock = new object();
        private static readonly ConditionalWeakTable<Control, RoundedBinding> RoundedControls = new ConditionalWeakTable<Control, RoundedBinding>();
        private static readonly ConditionalWeakTable<Control, InputBinding> InputControls = new ConditionalWeakTable<Control, InputBinding>();
        private static readonly ConditionalWeakTable<Form, WindowBinding> WindowBindings = new ConditionalWeakTable<Form, WindowBinding>();
        private static readonly ConditionalWeakTable<Button, ButtonBinding> ButtonBindings = new ConditionalWeakTable<Button, ButtonBinding>();
        private static readonly ConditionalWeakTable<ComboBox, ComboBinding> ComboBindings = new ConditionalWeakTable<ComboBox, ComboBinding>();
        private static readonly ConditionalWeakTable<CheckBox, CheckBinding> CheckBindings = new ConditionalWeakTable<CheckBox, CheckBinding>();
        private static readonly ConditionalWeakTable<TreeView, TreeCheckBinding> TreeCheckBindings = new ConditionalWeakTable<TreeView, TreeCheckBinding>();
        private static readonly ConditionalWeakTable<Control, NativeControlBinding> NativeControlBindings = new ConditionalWeakTable<Control, NativeControlBinding>();
        private static readonly ThemePalette DarkPalette = ThemePalette.Create(true);
        private static readonly ThemePalette LightPalette = ThemePalette.Create(false);
        private static readonly string UiFontFamily = ResolveUiFontFamily();
        private static string currentTheme = "System";
        private static ThemePalette currentPalette = LightPalette;
        private static bool initialized;
        private static Image watermarkImage;
        private static bool watermarkLoadAttempted;

        public static string CurrentTheme { get { return currentTheme; } }
        public static ThemePalette CurrentPalette { get { return currentPalette; } }
        public static bool IsInitialized { get { return initialized; } }

        public static void Initialize(string theme)
        {
            SetCurrentTheme(theme);
            AppIcon.Initialize();
            LoadWatermark();
            initialized = true;
        }

        public static void EnsureInitialized(string theme)
        {
            string normalized = String.Equals(theme, "Dark", StringComparison.OrdinalIgnoreCase) ? "Dark"
                : String.Equals(theme, "Light", StringComparison.OrdinalIgnoreCase) ? "Light" : "System";
            if (!initialized || !String.Equals(currentTheme, normalized, StringComparison.Ordinal)) Initialize(normalized);
        }

        public static void PrepareForm(Form form)
        {
            if (form == null) return;
            EnsureInitialized(CurrentTheme);
            form.BackColor = CurrentPalette.WindowBackground;
            form.ForeColor = CurrentPalette.TextPrimary;
            form.Font = UiFont(ThemeFontRole.Body);
            AppIcon.Apply(form);
            ApplyWindowChrome(form, CurrentTheme);
        }

        public static void PrepareForFirstShow(Form form, string theme)
        {
            if (form == null) return;
            SetCurrentTheme(theme);
            PrepareForm(form);
            if (!form.IsHandleCreated)
            {
                IntPtr unused = form.Handle;
            }
            ApplyWindowChrome(form, theme);
        }

        private static void SetCurrentTheme(string theme)
        {
            currentTheme = String.Equals(theme, "Dark", StringComparison.OrdinalIgnoreCase) ? "Dark"
                : String.Equals(theme, "Light", StringComparison.OrdinalIgnoreCase) ? "Light" : "System";
            currentPalette = PaletteFor(currentTheme);
        }

        public static bool IsDark(string theme)
        {
            if (String.Equals(theme, "Dark", StringComparison.OrdinalIgnoreCase)) return true;
            if (String.Equals(theme, "Light", StringComparison.OrdinalIgnoreCase)) return false;
            try
            {
                object value = Registry.GetValue(@"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme", 1);
                return Convert.ToInt32(value) == 0;
            }
            catch { return false; }
        }

        public static ThemePalette PaletteFor(string theme) { return IsDark(theme) ? DarkPalette : LightPalette; }

        public static Font UiFont(ThemeFontRole role)
        {
            switch (role)
            {
                case ThemeFontRole.Minor: return new Font(UiFontFamily, 8.5f, FontStyle.Regular, GraphicsUnit.Point);
                case ThemeFontRole.PanelTitle: return new Font(UiFontFamily, 11f, FontStyle.Bold, GraphicsUnit.Point);
                case ThemeFontRole.AppTitle: return new Font(UiFontFamily, 13f, FontStyle.Bold, GraphicsUnit.Point);
                case ThemeFontRole.Control: return new Font(UiFontFamily, 9.25f, FontStyle.Regular, GraphicsUnit.Point);
                default: return new Font(UiFontFamily, 9.5f, FontStyle.Regular, GraphicsUnit.Point);
            }
        }

        public static ToolTip CreateToolTip()
        {
            ToolTip toolTip = new ToolTip
            {
                AutoPopDelay = 12000,
                InitialDelay = 350,
                ReshowDelay = 100,
                ShowAlways = true,
                OwnerDraw = true
            };
            Font font = UiFont(ThemeFontRole.Minor);
            toolTip.Popup += delegate(object sender, PopupEventArgs e)
            {
                ThemePalette palette = CurrentPalette;
                toolTip.BackColor = palette.SurfaceRaised;
                toolTip.ForeColor = palette.TextPrimary;
                string value = toolTip.GetToolTip(e.AssociatedControl) ?? "";
                Size measured = TextRenderer.MeasureText(value, font, new Size(520, 0),
                    TextFormatFlags.WordBreak | TextFormatFlags.NoPrefix | TextFormatFlags.NoPadding);
                e.ToolTipSize = new Size(Math.Max(48, measured.Width + Space16), Math.Max(28, measured.Height + Space12));
            };
            toolTip.Draw += delegate(object sender, DrawToolTipEventArgs e)
            {
                ThemePalette palette = CurrentPalette;
                using (SolidBrush background = new SolidBrush(palette.SurfaceRaised))
                    e.Graphics.FillRectangle(background, e.Bounds);
                e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
                RectangleF surface = new RectangleF(0.5f, 0.5f, Math.Max(1, e.Bounds.Width - 1f), Math.Max(1, e.Bounds.Height - 1f));
                using (GraphicsPath path = RoundedPath(surface, ControlRadius))
                using (SolidBrush background = new SolidBrush(palette.SurfaceRaised))
                    e.Graphics.FillPath(background, path);
                using (GraphicsPath path = RoundedPath(surface, ControlRadius))
                using (Pen border = new Pen(palette.InputBorder))
                    e.Graphics.DrawPath(border, path);
                Rectangle textBounds = Rectangle.Inflate(e.Bounds, -Space8, -Space4);
                TextRenderer.DrawText(e.Graphics, e.ToolTipText, font, textBounds, palette.TextPrimary,
                    TextFormatFlags.WordBreak | TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix);
            };
            toolTip.Disposed += delegate { font.Dispose(); };
            return toolTip;
        }

        public static Font UiFont(ThemeFontRole role, FontStyle style)
        {
            using (Font source = UiFont(role)) return new Font(source.FontFamily, source.Size, style, GraphicsUnit.Point);
        }

        public static Color StatusColor(ThemeStatusColor status, string theme)
        {
            ThemePalette palette = PaletteFor(theme);
            switch (status)
            {
                case ThemeStatusColor.Orange: return palette.StatusOrange;
                case ThemeStatusColor.Red: return palette.StatusRed;
                case ThemeStatusColor.Purple: return palette.StatusPurple;
                case ThemeStatusColor.Green: return palette.StatusGreen;
                case ThemeStatusColor.Blue: return palette.StatusBlue;
                default: return palette.StatusWhite;
            }
        }

        public static void Apply(Control root, string theme)
        {
            Apply(root, theme, null);
        }

        public static void Apply(Control root, string theme, Action whileRedrawSuspended)
        {
            if (root == null) return;
            SetCurrentTheme(theme);
            ThemePalette palette = CurrentPalette;
            bool suppressRedraw = root.IsHandleCreated;
            List<Control> layoutControls = CollectControls(root);
            if (suppressRedraw) SendMessage(root.Handle, WmSetRedraw, IntPtr.Zero, IntPtr.Zero);
            foreach (Control control in layoutControls) control.SuspendLayout();
            try
            {
                ApplyControl(root, CurrentTheme, palette);
                Form form = root as Form;
                if (form != null) ApplyWindowChrome(form, CurrentTheme);
                if (whileRedrawSuspended != null) whileRedrawSuspended();
            }
            finally
            {
                for (int index = layoutControls.Count - 1; index >= 0; index--)
                    layoutControls[index].ResumeLayout(false);
                root.PerformLayout();
                if (suppressRedraw && root.IsHandleCreated)
                {
                    SendMessage(root.Handle, WmSetRedraw, new IntPtr(1), IntPtr.Zero);
                    RedrawWindow(root.Handle, IntPtr.Zero, IntPtr.Zero,
                        RdwInvalidate | RdwErase | RdwAllChildren | RdwFrame | RdwUpdateNow);
                }
                else root.Invalidate(true);
            }
        }

        public static void ApplyWindowChrome(Form form, string theme)
        {
            if (form == null) return;
            WindowBinding binding = WindowBindings.GetValue(form, delegate(Form key)
            {
                WindowBinding created = new WindowBinding();
                key.HandleCreated += delegate { ApplyNativeWindowChrome(key, created.Theme); };
                return created;
            });
            binding.Theme = theme;
            if (form.IsHandleCreated) ApplyNativeWindowChrome(form, theme);
        }

        public static void DrawWatermark(Graphics graphics, Rectangle bounds)
        {
            DrawWatermark(graphics, bounds, CurrentPalette);
        }

        public static void DrawWatermark(Graphics graphics, Rectangle bounds, ThemePalette palette)
        {
            Image logo = LoadWatermark();
            if (logo == null || bounds.Width < 120 || bounds.Height < 90) return;
            float scale = Math.Min(bounds.Width * 0.92f / logo.Width, bounds.Height * 0.92f / logo.Height);
            if (scale <= 0) return;
            int width = Math.Max(1, (int)Math.Round(logo.Width * scale));
            int height = Math.Max(1, (int)Math.Round(logo.Height * scale));
            Rectangle destination = new Rectangle(bounds.Left + (bounds.Width - width) / 2, bounds.Top + (bounds.Height - height) / 2, width, height);
            if (palette == null) palette = CurrentPalette;
            ColorMatrix matrix = new ColorMatrix();
            matrix.Matrix33 = palette.WatermarkOpacity;
            using (ImageAttributes attributes = new ImageAttributes())
            {
                attributes.SetColorKey(Color.Black, Color.FromArgb(24, 24, 24), ColorAdjustType.Bitmap);
                attributes.SetColorMatrix(matrix, ColorMatrixFlag.Default, ColorAdjustType.Bitmap);
                InterpolationMode previousInterpolation = graphics.InterpolationMode;
                CompositingQuality previousCompositing = graphics.CompositingQuality;
                PixelOffsetMode previousPixelOffset = graphics.PixelOffsetMode;
                graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                graphics.CompositingQuality = CompositingQuality.HighQuality;
                graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                graphics.DrawImage(logo, destination, 0, 0, logo.Width, logo.Height, GraphicsUnit.Pixel, attributes);
                graphics.InterpolationMode = previousInterpolation;
                graphics.CompositingQuality = previousCompositing;
                graphics.PixelOffsetMode = previousPixelOffset;
            }
        }

        private static List<Control> CollectControls(Control root)
        {
            List<Control> controls = new List<Control>();
            CollectControls(root, controls);
            return controls;
        }

        private static void CollectControls(Control root, List<Control> controls)
        {
            controls.Add(root);
            foreach (Control child in root.Controls) CollectControls(child, controls);
        }

        public static GraphicsPath RoundedPath(Rectangle bounds, int radius)
        {
            GraphicsPath path = new GraphicsPath();
            radius = Math.Max(1, Math.Min(radius, Math.Max(1, Math.Min(bounds.Width, bounds.Height) / 2)));
            int diameter = Math.Max(2, radius * 2);
            Rectangle arc = new Rectangle(bounds.Location, new Size(diameter, diameter));
            path.AddArc(arc, 180, 90);
            arc.X = bounds.Right - diameter;
            path.AddArc(arc, 270, 90);
            arc.Y = bounds.Bottom - diameter;
            path.AddArc(arc, 0, 90);
            arc.X = bounds.Left;
            path.AddArc(arc, 90, 90);
            path.CloseFigure();
            return path;
        }

        public static GraphicsPath RoundedPath(RectangleF bounds, float radius)
        {
            GraphicsPath path = new GraphicsPath();
            radius = Math.Max(1f, Math.Min(radius, Math.Max(1f, Math.Min(bounds.Width, bounds.Height) / 2f)));
            float diameter = Math.Max(2f, radius * 2f);
            RectangleF arc = new RectangleF(bounds.Location, new SizeF(diameter, diameter));
            path.AddArc(arc, 180, 90);
            arc.X = bounds.Right - diameter;
            path.AddArc(arc, 270, 90);
            arc.Y = bounds.Bottom - diameter;
            path.AddArc(arc, 0, 90);
            arc.X = bounds.Left;
            path.AddArc(arc, 90, 90);
            path.CloseFigure();
            return path;
        }

        public static void ApplyRoundedRegion(Control control, int radius)
        {
            if (control == null) return;
            RoundedBinding binding = RoundedControls.GetValue(control, delegate(Control key)
            {
                RoundedBinding created = new RoundedBinding();
                key.Resize += delegate { SetRoundedRegion(key, Scaled(key, created.Radius)); };
                key.DpiChangedAfterParent += delegate { SetRoundedRegion(key, Scaled(key, created.Radius)); };
                return created;
            });
            binding.Radius = radius;
            SetRoundedRegion(control, Scaled(control, radius));
        }

        public static void StyleButton(Button button, string theme)
        {
            if (button == null) return;
            ThemePalette palette = PaletteFor(theme);
            ConfigureButtonPainter(button, theme, palette);
            string role = button.Tag as string;
            bool stopping = button.Text.StartsWith("STOP", StringComparison.OrdinalIgnoreCase);
            Color foreground = palette.TextPrimary;
            Color border = palette.BorderSubtle;
            Color background = palette.ButtonBackground;
            Color hover = palette.ButtonHover;
            Color pressed = palette.ButtonPressed;
            button.UseVisualStyleBackColor = false;
            button.FlatStyle = FlatStyle.Flat;
            button.FlatAppearance.BorderSize = 0;
            button.Padding = button.Height >= 30 ? new Padding(Space12, Space4, Space12, Space4) : new Padding(Space8, 1, Space8, 1);
            if (!button.Enabled)
            {
                background = palette.ButtonDisabled;
                foreground = palette.TextDisabled;
                hover = pressed = background;
            }
            else if (stopping || role == "danger")
            {
                foreground = border = palette.Error;
                hover = Blend(palette.Error, palette.ButtonBackground, palette.Dark ? 0.77f : 0.90f);
                pressed = Blend(palette.Error, palette.ButtonBackground, palette.Dark ? 0.66f : 0.82f);
            }
            else if (role == "launch" || role == "primary")
            {
                background = palette.PrimaryAction;
                foreground = palette.PrimaryActionForeground;
                border = palette.AccentHover;
                hover = palette.PrimaryActionHover;
                pressed = palette.PrimaryActionPressed;
            }
            else if (role == "waiting")
            {
                foreground = border = palette.Warning;
                hover = Blend(palette.Warning, palette.ButtonBackground, palette.Dark ? 0.76f : 0.88f);
                pressed = Blend(palette.Warning, palette.ButtonBackground, palette.Dark ? 0.64f : 0.80f);
            }
            else if (role == "success")
            {
                foreground = border = palette.Success;
                background = Blend(palette.Success, palette.ButtonBackground, palette.Dark ? 0.88f : 0.95f);
                hover = Blend(palette.Success, palette.ButtonBackground, palette.Dark ? 0.78f : 0.90f);
                pressed = Blend(palette.Success, palette.ButtonBackground, palette.Dark ? 0.68f : 0.84f);
            }
            else if (role == "reset")
            {
                foreground = palette.Error;
                border = Blend(palette.Error, palette.BorderSubtle, 0.48f);
            }
            button.BackColor = background;
            button.ForeColor = foreground;
            button.FlatAppearance.BorderColor = border;
            button.FlatAppearance.MouseOverBackColor = hover;
            button.FlatAppearance.MouseDownBackColor = pressed;
            // Do not clip the HWND to a rounded Region. Region clipping cuts off
            // GDI+ antialias pixels at fractional DPI and produces the jagged
            // corners RC3 replaces with a single inset owner-paint pass.
            if (button.Region != null) { button.Region.Dispose(); button.Region = null; }
            button.Invalidate();
        }

        public static void StyleChoiceButton(Button button, bool selected, string theme)
        {
            if (button == null) return;
            ThemePalette palette = PaletteFor(theme);
            ConfigureButtonPainter(button, theme, palette);
            button.UseVisualStyleBackColor = false;
            button.FlatStyle = FlatStyle.Flat;
            button.FlatAppearance.BorderSize = 0;
            button.Padding = new Padding(Space8, 1, Space8, 1);
            if (!button.Enabled)
            {
                button.BackColor = palette.ButtonDisabled;
                button.ForeColor = palette.TextDisabled;
                button.FlatAppearance.BorderColor = palette.BorderSubtle;
                button.FlatAppearance.MouseOverBackColor = button.FlatAppearance.MouseDownBackColor = palette.ButtonDisabled;
            }
            else if (selected)
            {
                button.BackColor = palette.ButtonActive;
                button.ForeColor = palette.Success;
                button.FlatAppearance.BorderColor = palette.Success;
                button.FlatAppearance.MouseOverBackColor = Blend(palette.Success, palette.ButtonActiveHover, palette.Dark ? 0.82f : 0.92f);
                button.FlatAppearance.MouseDownBackColor = Blend(palette.Success, palette.ButtonActivePressed, palette.Dark ? 0.70f : 0.86f);
            }
            else
            {
                button.BackColor = palette.ButtonBackground;
                button.ForeColor = palette.TextPrimary;
                button.FlatAppearance.BorderColor = palette.BorderSubtle;
                button.FlatAppearance.MouseOverBackColor = palette.ButtonHover;
                button.FlatAppearance.MouseDownBackColor = palette.ButtonPressed;
            }
            if (button.Region != null) { button.Region.Dispose(); button.Region = null; }
            button.Invalidate();
        }

        private static void ConfigureButtonPainter(Button button, string theme, ThemePalette palette)
        {
            FluentButton fluent = button as FluentButton;
            if (fluent != null)
            {
                fluent.Theme = theme;
                fluent.Palette = palette;
                return;
            }
            ButtonBinding binding = EnsureButtonPainter(button);
            binding.Theme = theme;
            binding.Palette = palette;
        }

        private static ButtonBinding EnsureButtonPainter(Button button)
        {
            return ButtonBindings.GetValue(button, delegate(Button key)
            {
                ButtonBinding created = new ButtonBinding { Theme = CurrentTheme, Palette = CurrentPalette };
                key.EnabledChanged += delegate { StyleButton(key, created.Theme); };
                key.MouseEnter += delegate { created.Hot = true; key.Invalidate(); };
                key.MouseLeave += delegate { created.Hot = false; created.Pressed = false; key.Invalidate(); };
                key.MouseDown += delegate(object sender, MouseEventArgs args) { if (args.Button == MouseButtons.Left) { created.Pressed = true; key.Invalidate(); } };
                key.MouseUp += delegate { created.Pressed = false; key.Invalidate(); };
                key.KeyDown += delegate(object sender, KeyEventArgs args)
                {
                    if (args.KeyCode == Keys.Space || args.KeyCode == Keys.Enter) { created.Pressed = true; key.Invalidate(); }
                };
                key.KeyUp += delegate { created.Pressed = false; key.Invalidate(); };
                key.GotFocus += delegate { key.Invalidate(); };
                key.LostFocus += delegate { created.Pressed = false; key.Invalidate(); };
                key.Paint += delegate(object sender, PaintEventArgs args)
                {
                    DrawButton(key, args.Graphics, created.Palette, created.Hot, created.Pressed);
                };
                return created;
            });
        }

        internal static void DrawButton(Button button, Graphics graphics, ThemePalette palette, bool hot, bool pressed)
        {
            if (palette == null) palette = CurrentPalette;
            GraphicsState paintState = graphics.Save();
            graphics.SetClip(button.ClientRectangle, CombineMode.Intersect);
            try
            {
            float dpiScale = Math.Max(1f, button.DeviceDpi / 96f);
            float radius = Math.Max(4f, ControlRadius * dpiScale);
            RectangleF bounds = new RectangleF(0.75f, 0.75f,
                Math.Max(1f, button.Width - 1.5f), Math.Max(1f, button.Height - 1.5f));
            Color background = !button.Enabled ? palette.ButtonDisabled
                : pressed ? button.FlatAppearance.MouseDownBackColor
                : hot ? button.FlatAppearance.MouseOverBackColor : button.BackColor;
            Color foreground = button.Enabled ? button.ForeColor : palette.TextDisabled;
            bool focused = button.Focused && button.Enabled;
            string role = button.Tag as string;
            bool semanticBorder = button.Text.StartsWith("STOP", StringComparison.OrdinalIgnoreCase)
                || role == "danger" || role == "launch" || role == "primary" || role == "waiting" || role == "success" || role == "reset";
            Color border = focused && !semanticBorder ? palette.BorderFocus
                : button.Enabled ? button.FlatAppearance.BorderColor : palette.BorderSubtle;
            float borderWidth = focused ? Math.Max(1.5f, button.DeviceDpi / 64f)
                : Math.Max(1f, button.DeviceDpi / 96f);
            graphics.SmoothingMode = SmoothingMode.AntiAlias;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            graphics.CompositingQuality = CompositingQuality.HighQuality;
            Color outside = palette.SurfacePrimary;
            if (button.Parent != null && button.Parent.BackColor.A == 255) outside = button.Parent.BackColor;
            using (SolidBrush outsideBrush = new SolidBrush(outside))
                graphics.FillRectangle(outsideBrush, button.ClientRectangle);
            using (GraphicsPath path = RoundedPath(bounds, radius))
            using (SolidBrush brush = new SolidBrush(background)) graphics.FillPath(brush, path);
            using (GraphicsPath path = RoundedPath(bounds, radius))
            using (Pen pen = new Pen(border, borderWidth))
            {
                pen.Alignment = PenAlignment.Inset;
                graphics.DrawPath(pen, path);
            }

            if (focused)
            {
                RectangleF focusBounds = RectangleF.Inflate(bounds, -2f * dpiScale, -2f * dpiScale);
                if (focusBounds.Width > 2f && focusBounds.Height > 2f)
                {
                    using (GraphicsPath focusPath = RoundedPath(focusBounds, Math.Max(2f, radius - 2f * dpiScale)))
                    using (Pen focusPen = new Pen(palette.BorderFocus, Math.Max(1f, dpiScale)))
                    {
                        focusPen.Alignment = PenAlignment.Inset;
                        graphics.DrawPath(focusPen, focusPath);
                    }
                }
            }

            TextFormatFlags flags = TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPrefix;
            if (button.TextAlign == ContentAlignment.MiddleLeft) flags |= TextFormatFlags.Left;
            else if (button.TextAlign == ContentAlignment.MiddleRight) flags |= TextFormatFlags.Right;
            else flags |= TextFormatFlags.HorizontalCenter;
            Rectangle textBounds = Rectangle.Inflate(Rectangle.Round(bounds), -Scaled(button, Space8), 0);
            TextRenderer.DrawText(graphics, button.Text, button.Font, textBounds, foreground, flags);
            }
            finally
            {
                graphics.Restore(paintState);
            }
        }

        internal static void DrawCardSurface(Graphics graphics, Rectangle bounds, ThemePalette palette, CardVisualRole role, int radius)
        {
            if (graphics == null || bounds.Width <= 0 || bounds.Height <= 0) return;
            if (palette == null) palette = CurrentPalette;
            Color top;
            Color bottom;
            Color border = palette.BorderSubtle;
            switch (role)
            {
                case CardVisualRole.Nested:
                    top = palette.NestedCardSurface;
                    bottom = Blend(palette.NestedCardSurface, palette.SurfacePrimary, palette.Dark ? 0.12f : 0.04f);
                    break;
                case CardVisualRole.Recommended:
                    top = Blend(palette.AccentPrimary, palette.NestedCardSurface, palette.Dark ? 0.82f : 0.93f);
                    bottom = palette.NestedCardSurface;
                    border = palette.AccentPrimary;
                    break;
                case CardVisualRole.Log:
                    top = palette.LogWriteBackground;
                    bottom = Blend(palette.LogWriteBackground, palette.Shadow, palette.Dark ? 0.12f : 0.02f);
                    border = palette.LogBorder;
                    break;
                default:
                    top = palette.PanelSurface;
                    bottom = Blend(palette.PanelSurface, palette.WindowBackground, palette.Dark ? 0.14f : 0.03f);
                    break;
            }
            graphics.SmoothingMode = SmoothingMode.AntiAlias;
            using (GraphicsPath path = RoundedPath(bounds, radius))
            using (LinearGradientBrush brush = new LinearGradientBrush(bounds, top, bottom, LinearGradientMode.Vertical))
                graphics.FillPath(brush, path);
            using (GraphicsPath path = RoundedPath(bounds, radius))
            using (Pen pen = new Pen(border))
                graphics.DrawPath(pen, path);
        }

        private static int Scaled(Control control, int logicalPixels)
        {
            int dpi = control == null ? 96 : Math.Max(96, control.DeviceDpi);
            return Math.Max(1, (int)Math.Round(logicalPixels * dpi / 96f));
        }

        private static string ResolveUiFontFamily()
        {
            foreach (string family in new[] { "Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI" })
            {
                try { using (Font candidate = new Font(family, 9f)) if (String.Equals(candidate.Name, family, StringComparison.OrdinalIgnoreCase)) return family; }
                catch { }
            }
            return "Segoe UI";
        }

        private static Image LoadWatermark()
        {
            lock (WatermarkLock)
            {
                if (watermarkLoadAttempted) return watermarkImage;
                watermarkLoadAttempted = true;
                string path = Path.Combine(ConfigStore.AppRoot, "splined-watermark.png");
                if (!File.Exists(path)) return null;
                try { using (Image source = Image.FromFile(path)) watermarkImage = new Bitmap(source); }
                catch { watermarkImage = null; }
                return watermarkImage;
            }
        }

        private static void ApplyControl(Control control, string theme, ThemePalette palette)
        {
            control.ForeColor = control.Enabled ? palette.TextPrimary : palette.TextDisabled;
            Form form = control as Form;
            FluentGroupBox fluentGroup = control as FluentGroupBox;
            FluentCardPanel fluentCard = control as FluentCardPanel;
            FluentCardTableLayoutPanel fluentTableCard = control as FluentCardTableLayoutPanel;
            if (form != null)
            {
                form.BackColor = palette.WindowBackground;
                form.Font = UiFont(ThemeFontRole.Body);
                AppIcon.Apply(form);
            }
            else if (fluentGroup != null)
            {
                fluentGroup.Palette = palette;
                fluentGroup.BackColor = palette.NestedCardSurface;
                fluentGroup.ForeColor = palette.TextPrimary;
            }
            else if (fluentCard != null)
            {
                fluentCard.Palette = palette;
                fluentCard.BackColor = SurfaceForRole(palette, fluentCard.VisualRole);
            }
            else if (fluentTableCard != null)
            {
                fluentTableCard.Palette = palette;
                fluentTableCard.BackColor = SurfaceForRole(palette, fluentTableCard.VisualRole);
            }
            else if ((control is Panel || control is TableLayoutPanel || control is FlowLayoutPanel)
                && String.Equals(control.Tag as string, "titlebar-surface", StringComparison.Ordinal)) control.BackColor = palette.TopNavigationSurface;
            else if ((control is Panel || control is TableLayoutPanel || control is FlowLayoutPanel) && String.Equals(control.Tag as string, "surface-raised", StringComparison.Ordinal)) control.BackColor = palette.SurfaceRaised;
            else if (control is TabPage) control.BackColor = palette.SurfacePrimary;
            else if (control.Parent is NumericUpDown) control.BackColor = palette.InputBackground;
            else if (control is FluentCheckBox)
                control.BackColor = ContainerSurface(control, palette);
            else if (control is InfoButton)
                control.BackColor = ContainerSurface(control, palette);
            else if (control is Label || control is CheckBox || control is RadioButton) control.BackColor = Color.Transparent;
            else if (control is SplitContainer) control.BackColor = palette.BorderSubtle;
            else if (control is Panel || control is TableLayoutPanel || control is FlowLayoutPanel)
                control.BackColor = ContainerSurface(control, palette);
            else control.BackColor = palette.SurfaceSecondary;

            TextBoxBase text = control as TextBoxBase;
            if (text != null)
            {
                text.BackColor = palette.InputBackground;
                text.ForeColor = palette.TextPrimary;
                FluentTextBox fluentText = text as FluentTextBox;
                if (fluentText != null)
                {
                    fluentText.Theme = theme;
                    fluentText.Palette = palette;
                    fluentText.BorderStyle = BorderStyle.None;
                }
                else if (!(text is RichTextBox)) text.BorderStyle = BorderStyle.FixedSingle;
                AttachInputFocus(text, theme);
            }
            RichTextBox rich = control as RichTextBox;
            if (rich != null) { rich.BorderStyle = BorderStyle.None; ApplyRoundedRegion(rich, CardRadius); }
            TreeView tree = control as TreeView;
            if (tree != null)
            {
                tree.BackColor = palette.TreeSurface;
                tree.ForeColor = palette.TextPrimary;
                tree.BorderStyle = BorderStyle.None;
                tree.LineColor = palette.BorderSubtle;
                tree.ItemHeight = Math.Max(20, (int)Math.Round(22f * tree.DeviceDpi / 96f));
                if (tree.CheckBoxes) ApplyTreeCheckImages(tree, palette);
            }
            ListView list = control as ListView;
            if (list != null) { list.BackColor = palette.TreeSurface; list.ForeColor = palette.TextPrimary; list.BorderStyle = BorderStyle.None; }
            NumericUpDown number = control as NumericUpDown;
            if (number != null)
            {
                number.BackColor = palette.InputBackground;
                number.ForeColor = palette.TextPrimary;
                number.BorderStyle = number is FluentNumericUpDown ? BorderStyle.None : BorderStyle.FixedSingle;
                AttachInputFocus(number, theme);
                FluentNumericUpDown fluentNumber = number as FluentNumericUpDown;
                if (fluentNumber != null) { fluentNumber.Theme = theme; fluentNumber.Palette = palette; }
            }
            ComboBox combo = control as ComboBox;
            if (combo != null)
            {
                combo.BackColor = palette.InputBackground;
                combo.ForeColor = palette.TextPrimary;
                combo.FlatStyle = FlatStyle.Flat;
                AttachInputFocus(combo, theme);
                FluentComboBox fluentCombo = combo as FluentComboBox;
                if (fluentCombo != null) { fluentCombo.Theme = theme; fluentCombo.Palette = palette; }
            }
            if (text != null && !(text is RichTextBox) && !(text is FluentTextBox)) ApplyRoundedRegion(text, 4);
            CheckBox check = control as CheckBox;
            if (check != null)
            {
                FluentCheckBox fluentCheck = check as FluentCheckBox;
                if (fluentCheck != null) { fluentCheck.Theme = theme; fluentCheck.Palette = palette; }
                else
                {
                    check.UseVisualStyleBackColor = false;
                    check.FlatStyle = FlatStyle.Flat;
                    check.FlatAppearance.BorderColor = palette.BorderSubtle;
                    check.FlatAppearance.CheckedBackColor = palette.CheckOnBackground;
                    check.FlatAppearance.MouseOverBackColor = palette.SurfaceHover;
                    AttachCheckDrawing(check, theme);
                }
            }
            Button button = control as Button;
            if (button != null && !(button is InfoButton)) StyleButton(button, theme);
            MenuStrip menu = control as MenuStrip;
            if (menu != null)
            {
                menu.BackColor = palette.TopNavigationSurface;
                menu.ForeColor = palette.TextPrimary;
                FluentMenuStrip fluentMenu = menu as FluentMenuStrip;
                if (fluentMenu != null) fluentMenu.Palette = palette;
                menu.Renderer = new FluentMenuRenderer(palette);
                menu.Padding = new Padding(Space8, Space4, Space8, Space4);
                foreach (ToolStripItem item in menu.Items) ApplyMenuItem(item, palette);
            }
            StatusStrip status = control as StatusStrip;
            if (status != null) { status.BackColor = palette.TopNavigationSurface; status.ForeColor = palette.TextSecondary; status.Renderer = new FluentMenuRenderer(palette); }
            ThemedTabControl tabs = control as ThemedTabControl;
            if (tabs != null) tabs.Palette = palette;
            WatermarkTableLayoutPanel watermarkTable = control as WatermarkTableLayoutPanel;
            if (watermarkTable != null) watermarkTable.Palette = palette;
            WatermarkFlowLayoutPanel watermarkFlow = control as WatermarkFlowLayoutPanel;
            if (watermarkFlow != null) watermarkFlow.Palette = palette;
            if (combo != null && !(combo is FluentComboBox)) AttachComboDrawing(combo, theme);
            ScrollableControl scrollable = control as ScrollableControl;
            if (control is TabControl || control is TreeView || control is ListView || control is TextBoxBase
                || (control is ComboBox && !(control is FluentComboBox))
                || (control is NumericUpDown && !(control is FluentNumericUpDown))
                || (scrollable != null && scrollable.AutoScroll))
                ApplyNativeControlTheme(control, palette.Dark);
            foreach (Control child in control.Controls) ApplyControl(child, theme, palette);
        }

        private static void AttachComboDrawing(ComboBox combo, string theme)
        {
            if (combo.DropDownStyle != ComboBoxStyle.DropDownList) return;
            ComboBinding binding = ComboBindings.GetValue(combo, delegate(ComboBox key)
            {
                ComboBinding created = new ComboBinding();
                key.DrawMode = DrawMode.OwnerDrawFixed;
                key.DrawItem += delegate(object sender, DrawItemEventArgs args)
                {
                    ThemePalette active = created.Palette ?? CurrentPalette;
                    Color background = (args.State & DrawItemState.Selected) == DrawItemState.Selected ? active.SurfaceHover : active.InputBackground;
                    using (SolidBrush brush = new SolidBrush(background)) args.Graphics.FillRectangle(brush, args.Bounds);
                    string value = args.Index >= 0 && args.Index < key.Items.Count ? Convert.ToString(key.Items[args.Index]) : key.Text;
                    Rectangle textBounds = new Rectangle(args.Bounds.Left + Space8, args.Bounds.Top, Math.Max(1, args.Bounds.Width - Space12), args.Bounds.Height);
                    TextRenderer.DrawText(args.Graphics, value, key.Font, textBounds, key.Enabled ? active.TextPrimary : active.TextDisabled,
                        TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPrefix);
                    // The selected-row surface is the focus cue. The native
                    // DrawFocusRectangle would reintroduce a sharp square ring.
                };
                return created;
            });
            binding.Palette = PaletteFor(theme);
            combo.Invalidate();
        }

        private static void AttachCheckDrawing(CheckBox check, string theme)
        {
            CheckBinding binding = CheckBindings.GetValue(check, delegate(CheckBox key)
            {
                CheckBinding created = new CheckBinding();
                key.Paint += delegate(object sender, PaintEventArgs args)
                {
                    if (key.Appearance == Appearance.Button) return;
                    ThemePalette active = created.Palette ?? CurrentPalette;
                    int boxSize = Math.Max(12, (int)Math.Round(14f * key.DeviceDpi / 96f));
                    Rectangle box = new Rectangle(1, Math.Max(1, (key.Height - boxSize) / 2), boxSize, boxSize);
                    args.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
                    using (GraphicsPath path = RoundedPath(box, 2))
                    using (SolidBrush fill = new SolidBrush(key.Checked ? active.CheckOnBackground : active.CheckOffBackground))
                        args.Graphics.FillPath(fill, path);
                    using (GraphicsPath path = RoundedPath(box, 2))
                    using (Pen pen = new Pen(key.Checked ? active.CheckOnBackground : active.CheckOffBackground)) args.Graphics.DrawPath(pen, path);
                    if (key.Checked)
                    {
                        using (Pen pen = new Pen(active.CheckGlyph, Math.Max(1.5f, key.DeviceDpi / 72f)))
                        {
                            pen.StartCap = LineCap.Round;
                            pen.EndCap = LineCap.Round;
                            args.Graphics.DrawLines(pen, new[]
                            {
                                new PointF(box.Left + box.Width * 0.22f, box.Top + box.Height * 0.53f),
                                new PointF(box.Left + box.Width * 0.43f, box.Top + box.Height * 0.74f),
                                new PointF(box.Left + box.Width * 0.80f, box.Top + box.Height * 0.27f)
                            });
                        }
                    }
                };
                return created;
            });
            binding.Palette = PaletteFor(theme);
            check.Invalidate();
        }

        private static void ApplyTreeCheckImages(TreeView tree, ThemePalette palette)
        {
            int size = Math.Max(14, Math.Min(24, Scaled(tree, 16)));
            TreeCheckBinding binding = TreeCheckBindings.GetValue(tree, delegate { return new TreeCheckBinding(); });
            if (binding.Images != null && binding.Size == size
                && binding.Off.ToArgb() == palette.CheckOffBackground.ToArgb()
                && binding.On.ToArgb() == palette.CheckOnBackground.ToArgb()
                && binding.Glyph.ToArgb() == palette.CheckGlyph.ToArgb()
                && binding.Background.ToArgb() == palette.TreeSurface.ToArgb()) return;

            ImageList images = new ImageList
            {
                ColorDepth = ColorDepth.Depth32Bit,
                ImageSize = new Size(size, size),
                TransparentColor = Color.Transparent
            };
            images.Images.Add(DrawTreeCheckImage(size, palette.TreeSurface, palette.CheckOffBackground, palette.CheckGlyph, false));
            images.Images.Add(DrawTreeCheckImage(size, palette.TreeSurface, palette.CheckOnBackground, palette.CheckGlyph, true));

            ImageList previous = binding.Images;
            tree.StateImageList = images;
            binding.Images = images;
            binding.Size = size;
            binding.Off = palette.CheckOffBackground;
            binding.On = palette.CheckOnBackground;
            binding.Glyph = palette.CheckGlyph;
            binding.Background = palette.TreeSurface;
            if (previous != null) previous.Dispose();
        }

        private static Bitmap DrawTreeCheckImage(int size, Color backgroundColor, Color fillColor, Color glyphColor, bool isChecked)
        {
            Bitmap bitmap = new Bitmap(size, size, PixelFormat.Format32bppArgb);
            using (Graphics graphics = Graphics.FromImage(bitmap))
            {
                graphics.Clear(backgroundColor);
                graphics.SmoothingMode = SmoothingMode.AntiAlias;
                graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
                float inset = Math.Max(0.75f, size / 20f);
                RectangleF bounds = new RectangleF(inset, inset, size - inset * 2f - 0.5f, size - inset * 2f - 0.5f);
                using (GraphicsPath path = RoundedPath(bounds, Math.Max(2f, size * 0.18f)))
                using (SolidBrush fill = new SolidBrush(fillColor))
                    graphics.FillPath(fill, path);
                if (isChecked)
                {
                    using (Pen check = new Pen(glyphColor, Math.Max(1.5f, size / 9f)))
                    {
                        check.StartCap = LineCap.Round;
                        check.EndCap = LineCap.Round;
                        check.LineJoin = LineJoin.Round;
                        graphics.DrawLines(check, new[]
                        {
                            new PointF(size * 0.24f, size * 0.52f),
                            new PointF(size * 0.43f, size * 0.70f),
                            new PointF(size * 0.78f, size * 0.29f)
                        });
                    }
                }
            }
            return bitmap;
        }

        private static void AttachInputFocus(Control control, string theme)
        {
            InputBinding binding = InputControls.GetValue(control, delegate(Control key)
            {
                InputBinding created = new InputBinding();
                key.Enter += delegate { key.BackColor = (created.Palette ?? CurrentPalette).InputFocusBackground; };
                key.Leave += delegate { key.BackColor = (created.Palette ?? CurrentPalette).InputBackground; };
                key.EnabledChanged += delegate
                {
                    ThemePalette active = created.Palette ?? CurrentPalette;
                    key.ForeColor = key.Enabled ? active.TextPrimary : active.TextDisabled;
                    key.BackColor = key.Enabled ? active.InputBackground : active.ButtonDisabled;
                };
                return created;
            });
            binding.Palette = PaletteFor(theme);
        }

        private static void ApplyMenuItem(ToolStripItem item, ThemePalette palette)
        {
            item.ForeColor = item.Enabled ? palette.TextPrimary : palette.TextDisabled;
            item.BackColor = item.Owner is MenuStrip ? palette.TopNavigationSurface : palette.SurfaceRaised;
            ToolStripMenuItem menu = item as ToolStripMenuItem;
            if (menu == null) return;
            bool topLevel = menu.Owner is MenuStrip;
            menu.Padding = topLevel
                ? new Padding(Space8, Space4, Space8, Space4)
                : new Padding(Space8, Space4, Space12, Space4);
            menu.Margin = topLevel ? Padding.Empty : new Padding(Space4, 1, Space4, 1);
            menu.DropDown.BackColor = palette.SurfaceRaised;
            menu.DropDown.Padding = new Padding(Space4);
            menu.DropDown.Renderer = new FluentMenuRenderer(palette);
            ToolStripDropDownMenu dropDownMenu = menu.DropDown as ToolStripDropDownMenu;
            if (dropDownMenu != null)
            {
                bool hasCheckedItem = false;
                foreach (ToolStripItem child in menu.DropDownItems)
                {
                    ToolStripMenuItem childMenu = child as ToolStripMenuItem;
                    if (childMenu != null && childMenu.Checked) { hasCheckedItem = true; break; }
                }
                dropDownMenu.ShowImageMargin = false;
                dropDownMenu.ShowCheckMargin = hasCheckedItem;
            }
            foreach (ToolStripItem child in menu.DropDownItems) ApplyMenuItem(child, palette);
        }

        private static bool IsInsideRaisedSurface(Control control)
        {
            Control parent = control == null ? null : control.Parent;
            while (parent != null && !(parent is Form) && !(parent is TabPage))
            {
                if (parent is FluentGroupBox || parent is FluentCardPanel || parent is FluentCardTableLayoutPanel) return true;
                parent = parent.Parent;
            }
            return false;
        }

        private static Color ContainerSurface(Control control, ThemePalette palette)
        {
            Control parent = control == null ? null : control.Parent;
            while (parent != null && !(parent is Form) && !(parent is TabPage))
            {
                FluentGroupBox group = parent as FluentGroupBox;
                if (group != null) return palette.NestedCardSurface;
                FluentCardPanel panel = parent as FluentCardPanel;
                if (panel != null) return SurfaceForRole(palette, panel.VisualRole);
                FluentCardTableLayoutPanel table = parent as FluentCardTableLayoutPanel;
                if (table != null) return SurfaceForRole(palette, table.VisualRole);
                parent = parent.Parent;
            }
            return palette.SurfacePrimary;
        }

        private static Color SurfaceForRole(ThemePalette palette, CardVisualRole role)
        {
            if (role == CardVisualRole.Nested || role == CardVisualRole.Recommended) return palette.NestedCardSurface;
            if (role == CardVisualRole.Log) return palette.LogWriteBackground;
            return palette.PanelSurface;
        }

        private static void ApplyNativeControlTheme(Control control, bool dark)
        {
            NativeControlBinding binding = NativeControlBindings.GetValue(control, delegate(Control key)
            {
                NativeControlBinding created = new NativeControlBinding();
                key.HandleCreated += delegate { ApplyNativeControlThemeNow(key, created.Dark); };
                return created;
            });
            binding.Dark = dark;
            if (control.IsHandleCreated) ApplyNativeControlThemeNow(control, dark);
        }

        private static void ApplyNativeControlThemeNow(Control control, bool dark)
        {
            try { SetWindowTheme(control.Handle, dark ? "DarkMode_Explorer" : "Explorer", null); }
            catch { }
        }

        private static void SetRoundedRegion(Control control, int radius)
        {
            if (control.Width <= 1 || control.Height <= 1 || radius <= 0) return;
            using (GraphicsPath path = RoundedPath(new Rectangle(0, 0, control.Width, control.Height), radius))
            {
                Region previous = control.Region;
                control.Region = new Region(path);
                if (previous != null) previous.Dispose();
            }
        }

        internal static Color Blend(Color foreground, Color background, float backgroundWeight)
        {
            float bg = Math.Max(0f, Math.Min(1f, backgroundWeight));
            float fg = 1f - bg;
            return Color.FromArgb((int)Math.Round(foreground.R * fg + background.R * bg), (int)Math.Round(foreground.G * fg + background.G * bg), (int)Math.Round(foreground.B * fg + background.B * bg));
        }

        private static void ApplyNativeWindowChrome(Form form, string theme)
        {
            if (form == null || !form.IsHandleCreated || Environment.OSVersion.Platform != PlatformID.Win32NT) return;
            ThemePalette palette = PaletteFor(theme);
            try
            {
                int dark = palette.Dark ? 1 : 0;
                if (DwmSetWindowAttribute(form.Handle, 20, ref dark, sizeof(int)) != 0) DwmSetWindowAttribute(form.Handle, 19, ref dark, sizeof(int));
                int caption = ColorTranslator.ToWin32(palette.TitlebarBackground);
                int text = ColorTranslator.ToWin32(palette.TextPrimary);
                int border = ColorTranslator.ToWin32(palette.BorderSubtle);
                DwmSetWindowAttribute(form.Handle, 35, ref caption, sizeof(int));
                DwmSetWindowAttribute(form.Handle, 36, ref text, sizeof(int));
                DwmSetWindowAttribute(form.Handle, 34, ref border, sizeof(int));
                int cornerPreference = 3;
                DwmSetWindowAttribute(form.Handle, 33, ref cornerPreference, sizeof(int));
                if (Environment.OSVersion.Version.Build >= 22000)
                {
                    int backdrop = 2;
                    DwmSetWindowAttribute(form.Handle, 38, ref backdrop, sizeof(int));
                }
            }
            catch { }
        }

        [DllImport("dwmapi.dll", PreserveSig = true)]
        private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

        [DllImport("uxtheme.dll", CharSet = CharSet.Unicode)]
        private static extern int SetWindowTheme(IntPtr hwnd, string subAppName, string subIdList);

        private const int WmSetRedraw = 0x000B;
        private const uint RdwInvalidate = 0x0001, RdwErase = 0x0004, RdwFrame = 0x0400, RdwAllChildren = 0x0080, RdwUpdateNow = 0x0100;

        [DllImport("user32.dll")]
        private static extern IntPtr SendMessage(IntPtr window, int message, IntPtr wParam, IntPtr lParam);

        [DllImport("user32.dll")]
        private static extern bool RedrawWindow(IntPtr window, IntPtr updateRectangle, IntPtr updateRegion, uint flags);
    }

    internal class FluentButton : Button
    {
        private bool hot;
        private bool pressed;

        internal string Theme { get; set; }
        internal ThemePalette Palette { get; set; }

        public FluentButton()
        {
            Theme = ThemeManager.CurrentTheme;
            Palette = ThemeManager.CurrentPalette;
            UseVisualStyleBackColor = false;
            FlatStyle = FlatStyle.Flat;
            FlatAppearance.BorderSize = 0;
            SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint
                | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
            UpdateStyles();
        }

        protected override void OnPaintBackground(PaintEventArgs pevent)
        {
            // The single owner-paint pass below draws the complete rounded surface.
        }

        protected override void OnPaint(PaintEventArgs pevent)
        {
            ThemeManager.DrawButton(this, pevent.Graphics, Palette, hot, pressed);
        }

        protected override void OnMouseEnter(EventArgs e) { base.OnMouseEnter(e); hot = true; Invalidate(); }
        protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); hot = false; pressed = false; Invalidate(); }
        protected override void OnMouseDown(MouseEventArgs mevent) { base.OnMouseDown(mevent); if (mevent.Button == MouseButtons.Left) { pressed = true; Invalidate(); } }
        protected override void OnMouseUp(MouseEventArgs mevent) { base.OnMouseUp(mevent); pressed = false; Invalidate(); }
        protected override void OnMouseCaptureChanged(EventArgs e) { base.OnMouseCaptureChanged(e); pressed = false; Invalidate(); }
        protected override void OnKeyDown(KeyEventArgs kevent) { base.OnKeyDown(kevent); if (kevent.KeyCode == Keys.Space || kevent.KeyCode == Keys.Enter) { pressed = true; Invalidate(); } }
        protected override void OnKeyUp(KeyEventArgs kevent) { base.OnKeyUp(kevent); pressed = false; Invalidate(); }
        protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
        protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); pressed = false; Invalidate(); }
        protected override void OnEnabledChanged(EventArgs e) { base.OnEnabledChanged(e); Invalidate(); }
        protected override void OnTextChanged(EventArgs e) { base.OnTextChanged(e); Invalidate(); }

        public override void NotifyDefault(bool value)
        {
            // Default-button emphasis is rendered by the same rounded focus geometry.
            base.NotifyDefault(false);
            Invalidate();
        }
    }

    /// <summary>
    /// Native text editing with theme-owned border geometry. Only WM_PAINT is
    /// decorated after Windows paints the text; no parent/shared DC is touched.
    /// </summary>
    internal class FluentTextBox : TextBox
    {
        private const int WmPaint = 0x000F;
        private ThemePalette palette;
        private string theme;

        internal string Theme { get { return theme; } set { theme = value; Invalidate(); } }
        internal ThemePalette Palette
        {
            get { return palette; }
            set
            {
                palette = value;
                BackColor = ActivePalette.InputBackground;
                ForeColor = ActivePalette.TextPrimary;
                Invalidate();
            }
        }
        private ThemePalette ActivePalette { get { return palette ?? ThemeManager.CurrentPalette; } }

        public FluentTextBox()
        {
            theme = ThemeManager.CurrentTheme;
            palette = ThemeManager.CurrentPalette;
            BorderStyle = BorderStyle.None;
            AutoSize = false;
            SetStyle(ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
        }

        protected override void WndProc(ref Message message)
        {
            base.WndProc(ref message);
            if (message.Msg == WmPaint && IsHandleCreated)
            {
                using (Graphics graphics = CreateGraphics()) DrawBorder(graphics);
            }
        }

        private void DrawBorder(Graphics graphics)
        {
            if (Width <= 2 || Height <= 2) return;
            ThemePalette active = ActivePalette;
            float dpi = Math.Max(1f, DeviceDpi / 96f);
            RectangleF bounds = new RectangleF(0.75f, 0.75f, Width - 1.5f, Height - 1.5f);
            GraphicsState state = graphics.Save();
            graphics.SetClip(ClientRectangle, CombineMode.Intersect);
            graphics.SmoothingMode = SmoothingMode.AntiAlias;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, Math.Max(3f, ThemeManager.ControlRadius * dpi)))
            using (Pen pen = new Pen(Focused ? active.InputFocusBorder : active.InputBorder, Math.Max(1f, dpi)))
            {
                pen.Alignment = PenAlignment.Inset;
                graphics.DrawPath(pen, path);
            }
            graphics.Restore(state);
        }

        protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
        protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }
        protected override void OnEnabledChanged(EventArgs e) { base.OnEnabledChanged(e); Invalidate(); }
    }

    /// <summary>
    /// Compact owner-painted DropDownList. The native ComboBox remains the
    /// behavioral/accessibility authority; RC3 replaces only its chrome and
    /// item rendering so the affordance is visible in every theme.
    /// </summary>
    internal class FluentComboBox : ComboBox
    {
        private const int WmPaint = 0x000F;
        private bool hot;
        private ThemePalette palette;
        private string theme;

        internal string Theme
        {
            get { return theme; }
            set { theme = value; Invalidate(); }
        }

        internal ThemePalette Palette
        {
            get { return palette; }
            set { palette = value; BackColor = ActivePalette.InputBackground; ForeColor = ActivePalette.TextPrimary; Invalidate(); }
        }

        private ThemePalette ActivePalette { get { return palette ?? ThemeManager.CurrentPalette; } }

        public FluentComboBox()
        {
            theme = ThemeManager.CurrentTheme;
            palette = ThemeManager.CurrentPalette;
            DropDownStyle = ComboBoxStyle.DropDownList;
            DrawMode = DrawMode.OwnerDrawFixed;
            FlatStyle = FlatStyle.Flat;
            IntegralHeight = false;
            MaxDropDownItems = 10;
            SetStyle(ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
        }

        private int Scale(int logicalPixels)
        {
            return Math.Max(1, (int)Math.Round(logicalPixels * Math.Max(96, DeviceDpi) / 96f));
        }

        private void UpdateMetrics()
        {
            ItemHeight = Scale(28);
            DropDownHeight = ItemHeight * Math.Max(2, MaxDropDownItems) + Scale(2);
        }

        protected override void OnHandleCreated(EventArgs e)
        {
            base.OnHandleCreated(e);
            UpdateMetrics();
        }

        protected override void OnFontChanged(EventArgs e)
        {
            base.OnFontChanged(e);
            if (IsHandleCreated) UpdateMetrics();
        }

        protected override void OnDrawItem(DrawItemEventArgs e)
        {
            ThemePalette active = ActivePalette;
            Color background = (e.State & DrawItemState.Selected) == DrawItemState.Selected
                ? active.SurfaceHover : active.SurfaceRaised;
            using (SolidBrush brush = new SolidBrush(background)) e.Graphics.FillRectangle(brush, e.Bounds);
            string value = e.Index >= 0 && e.Index < Items.Count ? Convert.ToString(Items[e.Index]) : Text;
            Rectangle textBounds = new Rectangle(e.Bounds.Left + Scale(10), e.Bounds.Top,
                Math.Max(1, e.Bounds.Width - Scale(18)), e.Bounds.Height);
            TextRenderer.DrawText(e.Graphics, value, Font, textBounds,
                Enabled ? active.TextPrimary : active.TextDisabled,
                TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPrefix);
        }

        protected override void WndProc(ref Message m)
        {
            base.WndProc(ref m);
            if (m.Msg == WmPaint)
            {
                using (Graphics graphics = CreateGraphics()) DrawClosedState(graphics);
            }
        }

        private void DrawClosedState(Graphics graphics)
        {
            if (Width <= 2 || Height <= 2) return;
            GraphicsState paintState = graphics.Save();
            graphics.SetClip(ClientRectangle, CombineMode.Intersect);
            try
            {
            ThemePalette active = ActivePalette;
            float dpi = Math.Max(1f, DeviceDpi / 96f);
            float radius = Math.Max(3f, ThemeManager.ControlRadius * dpi);
            RectangleF bounds = new RectangleF(0.75f, 0.75f, Width - 1.5f, Height - 1.5f);
            Color outside = Parent != null && Parent.BackColor.A == 255 ? Parent.BackColor : active.SurfacePrimary;
            Color fill = !Enabled ? active.ButtonDisabled
                : Focused || DroppedDown ? active.InputFocusBackground
                : hot ? active.SurfaceHover : active.InputBackground;
            Color border = Focused || DroppedDown ? active.InputFocusBorder : active.InputBorder;

            graphics.SmoothingMode = SmoothingMode.AntiAlias;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            using (SolidBrush outsideBrush = new SolidBrush(outside))
                graphics.FillRectangle(outsideBrush, ClientRectangle);
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, radius))
            using (SolidBrush brush = new SolidBrush(fill)) graphics.FillPath(brush, path);
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, radius))
            using (Pen pen = new Pen(border, Math.Max(1f, dpi)))
            {
                pen.Alignment = PenAlignment.Inset;
                graphics.DrawPath(pen, path);
            }

            int arrowWidth = Scale(30);
            Rectangle textBounds = new Rectangle(Scale(10), 0, Math.Max(1, Width - arrowWidth - Scale(12)), Height);
            TextRenderer.DrawText(graphics, Text, Font, textBounds,
                Enabled ? active.TextPrimary : active.TextDisabled,
                TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPrefix);

            float centerX = Width - arrowWidth / 2f;
            float centerY = Height / 2f + dpi;
            using (Pen chevron = new Pen(Enabled ? active.AccentPrimary : active.TextDisabled, Math.Max(1.5f, 1.6f * dpi)))
            {
                chevron.StartCap = LineCap.Round;
                chevron.EndCap = LineCap.Round;
                chevron.LineJoin = LineJoin.Round;
                graphics.DrawLines(chevron, new[]
                {
                    new PointF(centerX - 4f * dpi, centerY - 2f * dpi),
                    new PointF(centerX, centerY + 2f * dpi),
                    new PointF(centerX + 4f * dpi, centerY - 2f * dpi)
                });
            }
            }
            finally
            {
                graphics.Restore(paintState);
            }
        }

        protected override void OnMouseEnter(EventArgs e) { base.OnMouseEnter(e); hot = true; Invalidate(); }
        protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); hot = false; Invalidate(); }
        protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
        protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }
        protected override void OnDropDown(EventArgs e) { base.OnDropDown(e); Invalidate(); }
        protected override void OnDropDownClosed(EventArgs e) { base.OnDropDownClosed(e); Invalidate(); }
        protected override void OnSelectedIndexChanged(EventArgs e) { base.OnSelectedIndexChanged(e); Invalidate(); }
        protected override void OnEnabledChanged(EventArgs e) { base.OnEnabledChanged(e); Invalidate(); }
    }

    /// <summary>
    /// Opaque single-pass checkbox chrome. Native behavior/accessibility is
    /// retained, while the final paint fully covers its white system glyph.
    /// </summary>
    internal class FluentCheckBox : CheckBox
    {
        private ThemePalette palette;
        private string theme;

        internal string Theme { get { return theme; } set { theme = value; Invalidate(); } }
        internal ThemePalette Palette
        {
            get { return palette; }
            set
            {
                palette = value;
                ForeColor = ActivePalette.TextPrimary;
                Invalidate();
            }
        }
        private ThemePalette ActivePalette { get { return palette ?? ThemeManager.CurrentPalette; } }

        public FluentCheckBox()
        {
            theme = ThemeManager.CurrentTheme;
            palette = ThemeManager.CurrentPalette;
            UseVisualStyleBackColor = false;
            FlatStyle = FlatStyle.Flat;
            AutoEllipsis = true;
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            base.OnPaint(e);
            GraphicsState paintState = e.Graphics.Save();
            e.Graphics.SetClip(ClientRectangle, CombineMode.Intersect);
            using (SolidBrush background = new SolidBrush(BackColor))
                e.Graphics.FillRectangle(background, ClientRectangle);
            ThemePalette active = ActivePalette;
            int boxSize = Math.Max(14, (int)Math.Round(16f * DeviceDpi / 96f));
            int boxTop = Math.Max(1, (Height - boxSize) / 2);
            bool rightAligned = CheckAlign == ContentAlignment.TopRight
                || CheckAlign == ContentAlignment.MiddleRight || CheckAlign == ContentAlignment.BottomRight;
            Rectangle box = new Rectangle(rightAligned ? Math.Max(1, Width - boxSize - 1) : 1, boxTop, boxSize, boxSize);
            Color stateColor = CheckState == CheckState.Indeterminate ? active.Warning
                : Checked ? active.CheckOnBackground : active.CheckOffBackground;
            if (!Enabled) stateColor = ThemeManager.Blend(stateColor, active.ButtonDisabled, 0.58f);

            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            using (GraphicsPath path = ThemeManager.RoundedPath(box, Math.Max(2f, boxSize * 0.18f)))
            using (SolidBrush fill = new SolidBrush(stateColor))
                e.Graphics.FillPath(fill, path);

            if (Checked)
            {
                using (Pen check = new Pen(active.CheckGlyph, Math.Max(1.6f, DeviceDpi / 64f)))
                {
                    check.StartCap = LineCap.Round;
                    check.EndCap = LineCap.Round;
                    check.LineJoin = LineJoin.Round;
                    e.Graphics.DrawLines(check, new[]
                    {
                        new PointF(box.Left + box.Width * 0.22f, box.Top + box.Height * 0.52f),
                        new PointF(box.Left + box.Width * 0.43f, box.Top + box.Height * 0.73f),
                        new PointF(box.Left + box.Width * 0.80f, box.Top + box.Height * 0.28f)
                    });
                }
            }
            else if (CheckState == CheckState.Indeterminate)
            {
                using (Pen dash = new Pen(active.CheckGlyph, Math.Max(1.6f, DeviceDpi / 64f)))
                    e.Graphics.DrawLine(dash, box.Left + box.Width * 0.25f, box.Top + box.Height * 0.5f,
                        box.Right - box.Width * 0.25f, box.Top + box.Height * 0.5f);
            }

            int gap = Math.Max(5, (int)Math.Round(6f * DeviceDpi / 96f));
            Rectangle textBounds = rightAligned
                ? new Rectangle(0, 0, Math.Max(1, box.Left - gap), Height)
                : new Rectangle(box.Right + gap, 0, Math.Max(1, Width - box.Right - gap), Height);
            TextFormatFlags flags = TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPrefix;
            flags |= rightAligned ? TextFormatFlags.Right : TextFormatFlags.Left;
            TextRenderer.DrawText(e.Graphics, Text, Font, textBounds,
                Enabled ? ForeColor : active.TextDisabled, flags);
            e.Graphics.Restore(paintState);

        }

        protected override void OnCheckedChanged(EventArgs e) { base.OnCheckedChanged(e); Invalidate(); }
        protected override void OnCheckStateChanged(EventArgs e) { base.OnCheckStateChanged(e); Invalidate(); }
        protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
        protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }
        protected override void OnEnabledChanged(EventArgs e)
        {
            base.OnEnabledChanged(e);
            ForeColor = Enabled ? ActivePalette.TextPrimary : ActivePalette.TextDisabled;
            Invalidate();
        }
    }

    /// <summary>
    /// NumericUpDown with a themed overlay for its native spinner buttons.
    /// Value, keyboard, validation, and accessibility behavior remain native.
    /// </summary>
    internal class FluentNumericUpDown : NumericUpDown
    {
        private const int WmPaint = 0x000F;
        private readonly FluentSpinnerSurface spinner;
        private ThemePalette palette;
        private string theme;

        internal string Theme { get { return theme; } set { theme = value; InvalidateAll(); } }
        internal ThemePalette Palette { get { return palette; } set { palette = value; BackColor = ActivePalette.InputBackground; ForeColor = ActivePalette.TextPrimary; InvalidateAll(); } }
        internal ThemePalette ActivePalette { get { return palette ?? ThemeManager.CurrentPalette; } }

        public FluentNumericUpDown()
        {
            theme = ThemeManager.CurrentTheme;
            palette = ThemeManager.CurrentPalette;
            BorderStyle = BorderStyle.None;
            spinner = new FluentSpinnerSurface(this);
            Controls.Add(spinner);
            spinner.BringToFront();
            SetStyle(ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
        }

        internal int ScaleMetric(int logicalPixels)
        {
            return Math.Max(1, (int)Math.Round(logicalPixels * Math.Max(96, DeviceDpi) / 96f));
        }

        private void PositionSpinner()
        {
            int width = ScaleMetric(28);
            spinner.SetBounds(Math.Max(0, ClientSize.Width - width), 0, width, ClientSize.Height);
            spinner.BringToFront();
        }

        private void InvalidateAll()
        {
            Invalidate();
            spinner.Invalidate();
        }

        protected override void OnCreateControl()
        {
            base.OnCreateControl();
            PositionSpinner();
        }

        protected override void WndProc(ref Message message)
        {
            base.WndProc(ref message);
            if (message.Msg == WmPaint && IsHandleCreated)
            {
                using (Graphics graphics = CreateGraphics()) DrawBorder(graphics);
            }
        }

        private void DrawBorder(Graphics graphics)
        {
            if (Width <= 2 || Height <= 2) return;
            float dpi = Math.Max(1f, DeviceDpi / 96f);
            RectangleF bounds = new RectangleF(0.75f, 0.75f, Width - 1.5f, Height - 1.5f);
            GraphicsState state = graphics.Save();
            graphics.SetClip(ClientRectangle, CombineMode.Intersect);
            graphics.SmoothingMode = SmoothingMode.AntiAlias;
            graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, Math.Max(3f, ThemeManager.ControlRadius * dpi)))
            using (Pen pen = new Pen(Focused ? ActivePalette.InputFocusBorder : ActivePalette.InputBorder, Math.Max(1f, dpi)))
            {
                pen.Alignment = PenAlignment.Inset;
                graphics.DrawPath(pen, path);
            }
            graphics.Restore(state);
        }

        protected override void OnResize(EventArgs e)
        {
            base.OnResize(e);
            PositionSpinner();
        }

        protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); InvalidateAll(); }
        protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); InvalidateAll(); }
        protected override void OnEnabledChanged(EventArgs e) { base.OnEnabledChanged(e); InvalidateAll(); }

        private sealed class FluentSpinnerSurface : Control
        {
            private readonly FluentNumericUpDown owner;
            private int hotHalf = -1;
            private int pressedHalf = -1;

            public FluentSpinnerSurface(FluentNumericUpDown owner)
            {
                this.owner = owner;
                TabStop = false;
                SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint
                    | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
            }

            protected override void OnPaint(PaintEventArgs e)
            {
                ThemePalette active = owner.ActivePalette;
                using (SolidBrush background = new SolidBrush(owner.Enabled ? active.InputBackground : active.ButtonDisabled))
                    e.Graphics.FillRectangle(background, ClientRectangle);
                int half = Math.Max(1, Height / 2);
                for (int index = 0; index < 2; index++)
                {
                    Rectangle area = index == 0
                        ? new Rectangle(0, 0, Width, half)
                        : new Rectangle(0, half, Width, Height - half);
                    if (owner.Enabled && (pressedHalf == index || hotHalf == index))
                    {
                        using (SolidBrush brush = new SolidBrush(pressedHalf == index ? active.SurfacePressed : active.SurfaceHover))
                            e.Graphics.FillRectangle(brush, area);
                    }
                }
                using (Pen separator = new Pen(active.InputBorder))
                {
                    e.Graphics.DrawLine(separator, 0, 1, 0, Math.Max(1, Height - 2));
                    e.Graphics.DrawLine(separator, 1, half, Math.Max(1, Width - 2), half);
                    e.Graphics.DrawLine(separator, Math.Max(0, Width - 1), 1, Math.Max(0, Width - 1), Math.Max(1, Height - 2));
                    e.Graphics.DrawLine(separator, 1, 0, Math.Max(1, Width - 2), 0);
                    e.Graphics.DrawLine(separator, 1, Math.Max(0, Height - 1), Math.Max(1, Width - 2), Math.Max(0, Height - 1));
                }
                e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
                DrawChevron(e.Graphics, new PointF(Width / 2f, half / 2f + 1), true, active);
                DrawChevron(e.Graphics, new PointF(Width / 2f, half + (Height - half) / 2f - 1), false, active);
            }

            private void DrawChevron(Graphics graphics, PointF center, bool up, ThemePalette active)
            {
                float dpi = Math.Max(1f, owner.DeviceDpi / 96f);
                float direction = up ? 1f : -1f;
                using (Pen pen = new Pen(owner.Enabled ? active.AccentPrimary : active.TextDisabled, Math.Max(1.3f, 1.4f * dpi)))
                {
                    pen.StartCap = LineCap.Round;
                    pen.EndCap = LineCap.Round;
                    pen.LineJoin = LineJoin.Round;
                    graphics.DrawLines(pen, new[]
                    {
                        new PointF(center.X - 3f * dpi, center.Y + direction * 1.5f * dpi),
                        new PointF(center.X, center.Y - direction * 1.5f * dpi),
                        new PointF(center.X + 3f * dpi, center.Y + direction * 1.5f * dpi)
                    });
                }
            }

            protected override void OnMouseMove(MouseEventArgs e)
            {
                base.OnMouseMove(e);
                int next = e.Y < Height / 2 ? 0 : 1;
                if (next != hotHalf) { hotHalf = next; Invalidate(); }
            }

            protected override void OnMouseLeave(EventArgs e)
            {
                base.OnMouseLeave(e);
                hotHalf = pressedHalf = -1;
                Invalidate();
            }

            protected override void OnMouseDown(MouseEventArgs e)
            {
                base.OnMouseDown(e);
                if (!owner.Enabled || e.Button != MouseButtons.Left) return;
                pressedHalf = e.Y < Height / 2 ? 0 : 1;
                owner.Focus();
                if (pressedHalf == 0) owner.UpButton(); else owner.DownButton();
                Invalidate();
            }

            protected override void OnMouseUp(MouseEventArgs e)
            {
                base.OnMouseUp(e);
                pressedHalf = -1;
                Invalidate();
            }
        }
    }

    internal class FluentForm : Form
    {
        public FluentForm()
        {
            SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
            UpdateStyles();
            ThemeManager.PrepareForm(this);
        }
    }

    internal sealed class FluentMenuColorTable : ProfessionalColorTable
    {
        private readonly ThemePalette palette;
        public FluentMenuColorTable(ThemePalette palette) { this.palette = palette; UseSystemColors = false; }
        public override Color MenuStripGradientBegin { get { return palette.TopNavigationSurface; } }
        public override Color MenuStripGradientEnd { get { return palette.TopNavigationSurface; } }
        public override Color ToolStripDropDownBackground { get { return palette.SurfaceRaised; } }
        public override Color ImageMarginGradientBegin { get { return palette.SurfaceRaised; } }
        public override Color ImageMarginGradientMiddle { get { return palette.SurfaceRaised; } }
        public override Color ImageMarginGradientEnd { get { return palette.SurfaceRaised; } }
        public override Color MenuItemSelected { get { return palette.SurfaceHover; } }
        public override Color MenuItemSelectedGradientBegin { get { return palette.SurfaceHover; } }
        public override Color MenuItemSelectedGradientEnd { get { return palette.SurfaceHover; } }
        public override Color MenuItemPressedGradientBegin { get { return palette.SurfacePressed; } }
        public override Color MenuItemPressedGradientMiddle { get { return palette.SurfacePressed; } }
        public override Color MenuItemPressedGradientEnd { get { return palette.SurfacePressed; } }
        public override Color MenuBorder { get { return palette.BorderSubtle; } }
        public override Color MenuItemBorder { get { return palette.BorderSubtle; } }
        public override Color SeparatorDark { get { return palette.BorderSubtle; } }
        public override Color SeparatorLight { get { return palette.SurfaceRaised; } }
        public override Color ToolStripBorder { get { return palette.BorderSubtle; } }
        public override Color StatusStripGradientBegin { get { return palette.TopNavigationSurface; } }
        public override Color StatusStripGradientEnd { get { return palette.TopNavigationSurface; } }
    }

    internal sealed class FluentMenuStrip : MenuStrip
    {
        private ThemePalette palette;

        internal ThemePalette Palette
        {
            get { return palette ?? ThemeManager.CurrentPalette; }
            set { palette = value; Invalidate(); }
        }

        public FluentMenuStrip()
        {
            AutoSize = true;
            GripStyle = ToolStripGripStyle.Hidden;
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            base.OnPaint(e);
            ThemePalette active = Palette;
            foreach (ToolStripItem item in Items)
            {
                if (!item.Available || String.IsNullOrEmpty(item.Text)) continue;
                TextRenderer.DrawText(e.Graphics, item.Text, item.Font, item.Bounds,
                    item.Enabled ? active.TextPrimary : active.TextDisabled,
                    TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter
                    | TextFormatFlags.SingleLine | TextFormatFlags.NoPrefix);
            }
        }
    }

    internal sealed class FluentMenuRenderer : ToolStripProfessionalRenderer
    {
        private readonly ThemePalette palette;
        internal const int PopupRadius = 6;
        internal const int HighlightRadius = 5;
        internal const int CheckRadius = 4;

        public FluentMenuRenderer(ThemePalette palette) : base(new FluentMenuColorTable(palette))
        {
            this.palette = palette;
            RoundedEdges = true;
        }

        internal static Rectangle HighlightBounds(Size size, bool topLevel)
        {
            int horizontalInset = topLevel ? 2 : 3;
            int verticalInset = topLevel ? 2 : 1;
            return new Rectangle(horizontalInset, verticalInset,
                Math.Max(1, size.Width - horizontalInset * 2 - 1),
                Math.Max(1, size.Height - verticalInset * 2 - 1));
        }

        protected override void OnRenderToolStripBackground(ToolStripRenderEventArgs e)
        {
            if (!(e.ToolStrip is ToolStripDropDown))
            {
                base.OnRenderToolStripBackground(e);
                return;
            }
            Rectangle bounds = new Rectangle(0, 0, Math.Max(1, e.ToolStrip.Width - 1), Math.Max(1, e.ToolStrip.Height - 1));
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, PopupRadius))
            using (SolidBrush brush = new SolidBrush(palette.SurfaceRaised))
                e.Graphics.FillPath(brush, path);
            using (GraphicsPath regionPath = ThemeManager.RoundedPath(bounds, PopupRadius))
            {
                Region previous = e.ToolStrip.Region;
                e.ToolStrip.Region = new Region(regionPath);
                if (previous != null) previous.Dispose();
            }
        }

        protected override void OnRenderToolStripBorder(ToolStripRenderEventArgs e)
        {
            if (!(e.ToolStrip is ToolStripDropDown))
            {
                base.OnRenderToolStripBorder(e);
                return;
            }
            Rectangle bounds = new Rectangle(0, 0, Math.Max(1, e.ToolStrip.Width - 1), Math.Max(1, e.ToolStrip.Height - 1));
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, PopupRadius))
            using (Pen pen = new Pen(palette.BorderSubtle))
            {
                pen.Alignment = PenAlignment.Inset;
                e.Graphics.DrawPath(pen, path);
            }
        }

        protected override void OnRenderImageMargin(ToolStripRenderEventArgs e)
        {
            if (e.ToolStrip is ToolStripDropDown)
            {
                using (SolidBrush brush = new SolidBrush(palette.SurfaceRaised)) e.Graphics.FillRectangle(brush, e.AffectedBounds);
                return;
            }
            base.OnRenderImageMargin(e);
        }

        protected override void OnRenderMenuItemBackground(ToolStripItemRenderEventArgs e)
        {
            bool topLevel = e.Item.Owner is MenuStrip;
            bool active = e.Item.Selected || e.Item.Pressed;
            ToolStripMenuItem menu = e.Item as ToolStripMenuItem;
            if (menu != null && menu.DropDown.Visible) active = true;
            if (!active) return;

            Rectangle bounds = HighlightBounds(e.Item.Size, topLevel);
            Color background = e.Item.Pressed || (menu != null && menu.DropDown.Visible)
                ? palette.SurfacePressed : palette.SurfaceHover;
            Color border = e.Item.Pressed || (menu != null && menu.DropDown.Visible)
                ? palette.BorderFocus : palette.BorderSubtle;
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, HighlightRadius))
            using (SolidBrush brush = new SolidBrush(background)) e.Graphics.FillPath(brush, path);
            using (GraphicsPath path = ThemeManager.RoundedPath(bounds, HighlightRadius))
            using (Pen pen = new Pen(border))
            {
                pen.Alignment = PenAlignment.Inset;
                e.Graphics.DrawPath(pen, path);
            }
        }

        protected override void OnRenderItemText(ToolStripItemTextRenderEventArgs e)
        {
            // ToolStrip can retain its native/default text color during the first
            // layout pass when it is hosted inside the custom title surface.
            // Always paint menu text from the active palette so File/View/Status/
            // Help are visible on the first themed frame as well as in popups.
            e.TextColor = e.Item.Enabled ? palette.TextPrimary : palette.TextDisabled;
            base.OnRenderItemText(e);
        }

        protected override void OnRenderItemCheck(ToolStripItemImageRenderEventArgs e)
        {
            int dpi = e.Item.Owner == null ? 96 : Math.Max(96, e.Item.Owner.DeviceDpi);
            int side = Math.Max(14, (int)Math.Round(16f * dpi / 96f));
            int left = Math.Max(5, (int)Math.Round(7f * dpi / 96f));
            Rectangle box = new Rectangle(left, Math.Max(1, (e.Item.Height - side) / 2), side, side);
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            using (GraphicsPath path = ThemeManager.RoundedPath(box, Math.Max(CheckRadius, (int)Math.Round(CheckRadius * dpi / 96f))))
            using (SolidBrush brush = new SolidBrush(palette.CheckOnBackground)) e.Graphics.FillPath(brush, path);
            using (GraphicsPath path = ThemeManager.RoundedPath(box, Math.Max(CheckRadius, (int)Math.Round(CheckRadius * dpi / 96f))))
            using (Pen pen = new Pen(palette.CheckOnBackground))
            {
                pen.Alignment = PenAlignment.Inset;
                e.Graphics.DrawPath(pen, path);
            }
            using (Pen check = new Pen(palette.CheckGlyph, Math.Max(1.7f, 2f * dpi / 96f)))
            {
                check.StartCap = LineCap.Round;
                check.EndCap = LineCap.Round;
                check.LineJoin = LineJoin.Round;
                PointF first = new PointF(box.Left + box.Width * 0.25f, box.Top + box.Height * 0.53f);
                PointF middle = new PointF(box.Left + box.Width * 0.43f, box.Top + box.Height * 0.70f);
                PointF last = new PointF(box.Left + box.Width * 0.76f, box.Top + box.Height * 0.32f);
                e.Graphics.DrawLines(check, new[] { first, middle, last });
            }
        }

        protected override void OnRenderSeparator(ToolStripSeparatorRenderEventArgs e)
        {
            int inset = ThemeManager.Space8;
            int y = e.Item.Height / 2;
            using (Pen pen = new Pen(palette.BorderSubtle))
                e.Graphics.DrawLine(pen, inset, y, Math.Max(inset, e.Item.Width - inset), y);
        }
    }

    internal sealed class FluentGroupBox : GroupBox
    {
        public ThemePalette Palette { get; set; }
        public FluentGroupBox()
        {
            SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
            Padding = new Padding(ThemeManager.Space12);
        }
        protected override void OnPaint(PaintEventArgs e)
        {
            ThemePalette palette = Palette ?? ThemeManager.CurrentPalette;
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            using (SolidBrush background = new SolidBrush(palette.NestedCardSurface)) e.Graphics.FillRectangle(background, ClientRectangle);
            Size textSize = TextRenderer.MeasureText(Text ?? "", Font, new Size(Int32.MaxValue, Int32.MaxValue), TextFormatFlags.NoPadding);
            int titleLeft = ThemeManager.Space12;
            int lineTop = Math.Max(7, textSize.Height / 2);
            Rectangle outline = new Rectangle(0, lineTop, Math.Max(1, Width - 1), Math.Max(1, Height - lineTop - 1));
            using (GraphicsPath path = ThemeManager.RoundedPath(outline, ThemeManager.CardRadius))
            using (Pen pen = new Pen(palette.BorderSubtle)) e.Graphics.DrawPath(pen, path);
            if (!String.IsNullOrEmpty(Text))
            {
                Rectangle titleBack = new Rectangle(titleLeft - 4, 0, textSize.Width + 8, textSize.Height + 1);
                using (SolidBrush brush = new SolidBrush(palette.NestedCardSurface)) e.Graphics.FillRectangle(brush, titleBack);
                TextRenderer.DrawText(e.Graphics, Text, Font, new Point(titleLeft, 0), Enabled ? palette.TextPrimary : palette.TextDisabled, TextFormatFlags.NoPadding | TextFormatFlags.NoPrefix);
            }
        }
    }

    internal sealed class FluentCardPanel : Panel
    {
        public ThemePalette Palette { get; set; }
        public CardVisualRole VisualRole { get; set; }
        public FluentCardPanel()
        {
            VisualRole = CardVisualRole.Panel;
            SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
            BorderStyle = BorderStyle.None;
        }
        protected override void OnPaintBackground(PaintEventArgs e)
        {
            ThemePalette palette = Palette ?? ThemeManager.CurrentPalette;
            using (SolidBrush background = new SolidBrush(palette.SurfacePrimary))
                e.Graphics.FillRectangle(background, ClientRectangle);
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            Rectangle bounds = new Rectangle(0, 0, Math.Max(1, Width - 1), Math.Max(1, Height - 1));
            ThemeManager.DrawCardSurface(e.Graphics, bounds, palette, VisualRole,
                VisualRole == CardVisualRole.Panel || VisualRole == CardVisualRole.Log ? ThemeManager.PanelRadius : ThemeManager.CardRadius);
        }
    }

    internal sealed class FluentCardTableLayoutPanel : TableLayoutPanel
    {
        public ThemePalette Palette { get; set; }
        public CardVisualRole VisualRole { get; set; }
        public FluentCardTableLayoutPanel()
        {
            VisualRole = CardVisualRole.Panel;
            SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
            BorderStyle = BorderStyle.None;
        }
        protected override void OnPaintBackground(PaintEventArgs e)
        {
            ThemePalette palette = Palette ?? ThemeManager.CurrentPalette;
            using (SolidBrush background = new SolidBrush(palette.SurfacePrimary))
                e.Graphics.FillRectangle(background, ClientRectangle);
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            Rectangle bounds = new Rectangle(0, 0, Math.Max(1, Width - 1), Math.Max(1, Height - 1));
            ThemeManager.DrawCardSurface(e.Graphics, bounds, palette, VisualRole,
                VisualRole == CardVisualRole.Panel || VisualRole == CardVisualRole.Log ? ThemeManager.PanelRadius : ThemeManager.CardRadius);
        }
    }

    internal sealed class WatermarkTableLayoutPanel : TableLayoutPanel
    {
        public ThemePalette Palette { get; set; }
        public WatermarkTableLayoutPanel() { DoubleBuffered = true; ResizeRedraw = true; }
        protected override void OnPaintBackground(PaintEventArgs e) { base.OnPaintBackground(e); ThemeManager.DrawWatermark(e.Graphics, ClientRectangle, Palette ?? ThemeManager.CurrentPalette); }
    }

    internal sealed class WatermarkFlowLayoutPanel : FlowLayoutPanel
    {
        public ThemePalette Palette { get; set; }
        public WatermarkFlowLayoutPanel() { DoubleBuffered = true; ResizeRedraw = true; }
        protected override void OnPaintBackground(PaintEventArgs e)
        {
            ThemePalette palette = Palette ?? ThemeManager.CurrentPalette;
            using (SolidBrush background = new SolidBrush(palette.SurfacePrimary))
                e.Graphics.FillRectangle(background, ClientRectangle);
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            Rectangle bounds = new Rectangle(0, 0, Math.Max(1, Width - 1), Math.Max(1, Height - 1));
            ThemeManager.DrawCardSurface(e.Graphics, bounds, palette, CardVisualRole.Panel, ThemeManager.PanelRadius);
            ThemeManager.DrawWatermark(e.Graphics, new Rectangle(ThemeManager.Space8, ThemeManager.Space8, Math.Max(1, Width - ThemeManager.Space16), Math.Max(1, Height - ThemeManager.Space16)), palette);
        }
    }

    internal sealed class ThemedTabControl : TabControl
    {
        public ThemePalette Palette { get; set; }
        private int hotIndex = -1;
        public ThemedTabControl()
        {
            SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
            DrawMode = TabDrawMode.OwnerDrawFixed;
            Padding = new Point(14, 5);
            SizeMode = TabSizeMode.Normal;
            SelectedIndexChanged += delegate { Invalidate(); };
        }
        protected override void OnMouseMove(MouseEventArgs e)
        {
            base.OnMouseMove(e);
            int next = -1;
            for (int index = 0; index < TabCount; index++)
                if (GetTabRect(index).Contains(e.Location)) { next = index; break; }
            if (next != hotIndex) { hotIndex = next; Invalidate(); }
        }
        protected override void OnMouseLeave(EventArgs e)
        {
            base.OnMouseLeave(e);
            hotIndex = -1;
            Invalidate();
        }
        protected override void OnDrawItem(DrawItemEventArgs e)
        {
            DrawTab(e.Graphics, e.Index);
        }
        protected override void OnPaint(PaintEventArgs e)
        {
            ThemePalette palette = Palette ?? ThemeManager.CurrentPalette;
            using (SolidBrush background = new SolidBrush(palette.SurfacePrimary))
                e.Graphics.FillRectangle(background, ClientRectangle);
            for (int index = 0; index < TabCount; index++) DrawTab(e.Graphics, index);
            Rectangle display = DisplayRectangle;
            using (Pen pen = new Pen(palette.BorderSubtle))
                e.Graphics.DrawLine(pen, 0, Math.Max(0, display.Top - 1), Math.Max(0, Width - 1), Math.Max(0, display.Top - 1));
        }
        private void DrawTab(Graphics graphics, int index)
        {
            ThemePalette palette = Palette ?? ThemeManager.CurrentPalette;
            bool selected = index == SelectedIndex;
            Rectangle bounds = GetTabRect(index);
            RectangleF surface = new RectangleF(bounds.Left + 1f, bounds.Top + 2f,
                Math.Max(1f, bounds.Width - 2f), Math.Max(1f, bounds.Height - 4f));
            if (selected || index == hotIndex)
            {
                graphics.SmoothingMode = SmoothingMode.AntiAlias;
                using (GraphicsPath path = ThemeManager.RoundedPath(surface, ThemeManager.ControlRadius))
                using (SolidBrush background = new SolidBrush(selected ? palette.SurfaceRaised : palette.SurfaceHover))
                    graphics.FillPath(background, path);
            }
            if (selected)
            {
                using (Pen accent = new Pen(palette.AccentPrimary, Math.Max(2f, DeviceDpi / 48f)))
                {
                    accent.StartCap = LineCap.Round;
                    accent.EndCap = LineCap.Round;
                    graphics.DrawLine(accent, bounds.Left + 8, bounds.Bottom - 2, bounds.Right - 8, bounds.Bottom - 2);
                }
            }
            TextRenderer.DrawText(graphics, TabPages[index].Text, Font, bounds, selected ? palette.TextPrimary : palette.TextSecondary,
                TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPrefix);
        }
    }

    internal sealed class InfoButton : Button
    {
        private readonly ToolTip toolTip;
        private bool hot;
        public InfoButton(string helpText)
        {
            Width = 22; Height = 22; TabStop = true; FlatStyle = FlatStyle.Flat; FlatAppearance.BorderSize = 0; Text = ""; Cursor = Cursors.Help;
            AccessibleName = "Information"; AccessibleDescription = helpText;
            toolTip = ThemeManager.CreateToolTip();
            toolTip.SetToolTip(this, helpText);
        }
        protected override void OnPaint(PaintEventArgs e)
        {
            GraphicsState paintState = e.Graphics.Save();
            e.Graphics.SetClip(ClientRectangle, CombineMode.Intersect);
            using (SolidBrush background = new SolidBrush(BackColor))
                e.Graphics.FillRectangle(background, ClientRectangle);
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            ThemePalette palette = ThemeManager.CurrentPalette;
            Rectangle circle = new Rectangle(2, 2, Math.Max(1, Width - 5), Math.Max(1, Height - 5));
            using (SolidBrush brush = new SolidBrush(hot ? palette.AccentHover : palette.InfoBackground)) e.Graphics.FillEllipse(brush, circle);
            using (Pen border = new Pen(hot || Focused ? palette.BorderFocus : palette.BorderSubtle, Focused ? 1.5f : 1f)) e.Graphics.DrawEllipse(border, circle);
            using (Font font = ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold))
            using (StringFormat format = new StringFormat())
            using (SolidBrush textBrush = new SolidBrush(palette.InfoForeground))
            {
                format.Alignment = StringAlignment.Center; format.LineAlignment = StringAlignment.Center;
                e.Graphics.DrawString("i", font, textBrush, new RectangleF(1, 0, Width - 3, Height - 3), format);
            }
            e.Graphics.Restore(paintState);
        }
        protected override void OnMouseEnter(EventArgs e) { base.OnMouseEnter(e); hot = true; Invalidate(); }
        protected override void OnMouseLeave(EventArgs e) { base.OnMouseLeave(e); hot = false; Invalidate(); }
    }
}
