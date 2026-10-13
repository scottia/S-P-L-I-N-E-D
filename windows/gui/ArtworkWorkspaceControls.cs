using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal sealed class NeonCardPanel : Panel
    {
        public bool SpectrumBorder { get; set; }
        public bool Selected { get; set; }
        public Color AccentColor { get; set; }
        public int Radius { get; set; }

        public NeonCardPanel()
        {
            DoubleBuffered = true;
            ResizeRedraw = true;
            Radius = 8;
            AccentColor = Color.Empty;
            Padding = new Padding(1);
            Margin = new Padding(4);
        }

        public void ActivateCard()
        {
            OnClick(EventArgs.Empty);
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            RectangleF bounds = new RectangleF(0.5f, 0.5f,
                Math.Max(1, Width - 1f), Math.Max(1, Height - 1f));
            using (GraphicsPath path = Rounded(bounds, Radius))
            using (SolidBrush background = new SolidBrush(palette.SurfacePrimary))
                e.Graphics.FillPath(background, path);

            if (SpectrumBorder)
            {
                using (LinearGradientBrush gradient = new LinearGradientBrush(
                    ClientRectangle,
                    Color.FromArgb(225, 39, 93, 255),
                    Color.FromArgb(225, 246, 52, 181),
                    LinearGradientMode.Horizontal))
                using (Pen border = new Pen(gradient, Selected ? 2.2f : 1.25f))
                using (GraphicsPath path = Rounded(new RectangleF(1f, 1f,
                    Math.Max(1, Width - 2f), Math.Max(1, Height - 2f)), Radius))
                    e.Graphics.DrawPath(border, path);
            }
            else
            {
                Color borderColor = Selected ? palette.BorderFocus
                    : AccentColor.IsEmpty ? palette.BorderSubtle : AccentColor;
                using (Pen border = new Pen(borderColor, Selected ? 2f : 1f))
                using (GraphicsPath path = Rounded(new RectangleF(1f, 1f,
                    Math.Max(1, Width - 2f), Math.Max(1, Height - 2f)), Radius))
                    e.Graphics.DrawPath(border, path);
            }
            base.OnPaint(e);
        }

        internal static GraphicsPath Rounded(RectangleF bounds, float radius)
        {
            float diameter = Math.Max(1, radius * 2);
            GraphicsPath path = new GraphicsPath();
            path.AddArc(bounds.Left, bounds.Top, diameter, diameter, 180, 90);
            path.AddArc(bounds.Right - diameter, bounds.Top, diameter, diameter, 270, 90);
            path.AddArc(bounds.Right - diameter, bounds.Bottom - diameter, diameter, diameter, 0, 90);
            path.AddArc(bounds.Left, bounds.Bottom - diameter, diameter, diameter, 90, 90);
            path.CloseFigure();
            return path;
        }
    }

    internal sealed class HorizontalWheelFlowPanel : FlowLayoutPanel
    {
        private const int SbBoth = 3;

        [DllImport("user32.dll")]
        private static extern bool ShowScrollBar(IntPtr handle, int bar, bool show);

        public HorizontalWheelFlowPanel()
        {
            AutoScroll = true;
            WrapContents = false;
            FlowDirection = FlowDirection.LeftToRight;
            DoubleBuffered = true;
        }

        protected override void OnHandleCreated(EventArgs e)
        {
            base.OnHandleCreated(e);
            HideNativeScrollbars();
        }

        protected override void OnLayout(LayoutEventArgs levent)
        {
            base.OnLayout(levent);
            HideNativeScrollbars();
        }

        protected override void OnSizeChanged(EventArgs e)
        {
            base.OnSizeChanged(e);
            HideNativeScrollbars();
        }

        protected override void OnMouseWheel(MouseEventArgs e)
        {
            int current = Math.Abs(AutoScrollPosition.X);
            int next = Math.Max(0, current - Math.Sign(e.Delta) * 92);
            AutoScrollPosition = new Point(next, 0);
            HideNativeScrollbars();
        }

        public void ScrollBy(int delta)
        {
            int current = Math.Abs(AutoScrollPosition.X);
            AutoScrollPosition = new Point(Math.Max(0, current + delta), 0);
            HideNativeScrollbars();
        }

        private void HideNativeScrollbars()
        {
            if (IsHandleCreated) ShowScrollBar(Handle, SbBoth, false);
        }
    }

    /// <summary>
    /// Keeps native scrolling semantics and keyboard accessibility while the
    /// operating-system scrollbar chrome stays out of the neon workspace.
    /// The wheel remains the primary compact-layout navigation affordance.
    /// </summary>
    internal sealed class HiddenScrollPanel : Panel
    {
        private const int SbBoth = 3;

        [DllImport("user32.dll")]
        private static extern bool ShowScrollBar(IntPtr handle, int bar, bool show);

        public HiddenScrollPanel()
        {
            AutoScroll = true;
            DoubleBuffered = true;
        }

        protected override void OnHandleCreated(EventArgs e)
        {
            base.OnHandleCreated(e);
            HideBars();
        }

        protected override void OnLayout(LayoutEventArgs levent)
        {
            base.OnLayout(levent);
            HideBars();
        }

        protected override void OnMouseWheel(MouseEventArgs e)
        {
            int current = Math.Abs(AutoScrollPosition.Y);
            int next = Math.Max(0, current - Math.Sign(e.Delta) * 72);
            AutoScrollPosition = new Point(0, next);
            HideBars();
        }

        private void HideBars()
        {
            if (IsHandleCreated) ShowScrollBar(Handle, SbBoth, false);
        }
    }

    internal sealed class ModernSpectrumSlider : Control
    {
        private int minimum;
        private int maximum = 100;
        private int value;
        private bool dragging;

        public event EventHandler ValueChanged;

        public int Minimum
        {
            get { return minimum; }
            set { minimum = value; if (maximum < minimum) maximum = minimum; Value = this.value; Invalidate(); }
        }

        public int Maximum
        {
            get { return maximum; }
            set { maximum = Math.Max(value, minimum); Value = this.value; Invalidate(); }
        }

        public int Value
        {
            get { return value; }
            set
            {
                int bounded = Math.Max(minimum, Math.Min(maximum, value));
                if (this.value == bounded) return;
                this.value = bounded;
                Invalidate();
                EventHandler changed = ValueChanged;
                if (changed != null) changed(this, EventArgs.Empty);
            }
        }

        public ModernSpectrumSlider()
        {
            DoubleBuffered = true;
            SetStyle(ControlStyles.Selectable, true);
            Height = 30;
            Cursor = Cursors.Hand;
            TabStop = true;
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            int left = 9;
            int right = Math.Max(left + 1, Width - 9);
            int center = Height / 2;
            Rectangle track = new Rectangle(left, center - 2, right - left, 5);
            using (GraphicsPath trackPath = NeonCardPanel.Rounded(track, 3))
            using (LinearGradientBrush gradient = new LinearGradientBrush(track,
                Color.FromArgb(25, 222, 237), Color.FromArgb(248, 65, 170),
                LinearGradientMode.Horizontal))
                e.Graphics.FillPath(gradient, trackPath);
            float fraction = maximum == minimum ? 0.5f
                : (value - minimum) / (float)(maximum - minimum);
            int x = left + (int)Math.Round((right - left) * fraction);
            using (SolidBrush shadow = new SolidBrush(Color.FromArgb(100, palette.BorderFocus)))
                e.Graphics.FillEllipse(shadow, x - 8, center - 8, 16, 16);
            using (SolidBrush thumb = new SolidBrush(Color.White))
                e.Graphics.FillEllipse(thumb, x - 5, center - 5, 10, 10);
            if (Focused)
            {
                using (Pen focus = new Pen(palette.BorderFocus))
                    e.Graphics.DrawRectangle(focus, 1, 1, Math.Max(1, Width - 3), Math.Max(1, Height - 3));
            }
        }

        protected override void OnMouseDown(MouseEventArgs e)
        {
            base.OnMouseDown(e);
            if (e.Button != MouseButtons.Left) return;
            Focus();
            dragging = true;
            SetFromX(e.X);
        }

        protected override void OnMouseMove(MouseEventArgs e)
        {
            base.OnMouseMove(e);
            if (dragging) SetFromX(e.X);
        }

        protected override void OnMouseUp(MouseEventArgs e)
        {
            dragging = false;
            base.OnMouseUp(e);
        }

        protected override void OnKeyDown(KeyEventArgs e)
        {
            if (e.KeyCode == Keys.Left || e.KeyCode == Keys.Down) { Value--; e.Handled = true; }
            else if (e.KeyCode == Keys.Right || e.KeyCode == Keys.Up) { Value++; e.Handled = true; }
            else if (e.KeyCode == Keys.Home) { Value = Minimum; e.Handled = true; }
            else if (e.KeyCode == Keys.End) { Value = Maximum; e.Handled = true; }
            base.OnKeyDown(e);
        }

        private void SetFromX(int x)
        {
            int width = Math.Max(1, Width - 18);
            float fraction = Math.Max(0f, Math.Min(1f, (x - 9) / (float)width));
            Value = minimum + (int)Math.Round((maximum - minimum) * fraction);
        }
    }

    internal sealed class TriStateFilterControl : Control
    {
        private TriStateFilterValue state;
        public event EventHandler StateChanged;
        public string Caption { get; set; }
        public TriStateFilterValue State
        {
            get { return state; }
            set { if (state == value) return; state = value; Invalidate(); OnStateChanged(); }
        }

        public TriStateFilterControl()
        {
            Caption = "Category";
            Height = 28;
            Cursor = Cursors.Hand;
            DoubleBuffered = true;
            AccessibleRole = AccessibleRole.CheckButton;
            TabStop = true;
        }

        protected override void OnClick(EventArgs e)
        {
            State = state == TriStateFilterValue.Neutral ? TriStateFilterValue.Included
                : state == TriStateFilterValue.Included ? TriStateFilterValue.Disregarded
                : TriStateFilterValue.Neutral;
            base.OnClick(e);
        }

        protected override void OnKeyDown(KeyEventArgs e)
        {
            if (e.KeyCode == Keys.Space || e.KeyCode == Keys.Enter)
            {
                OnClick(EventArgs.Empty);
                e.Handled = true;
            }
            base.OnKeyDown(e);
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            Rectangle box = new Rectangle(2, (Height - 18) / 2, 18, 18);
            Color border = state == TriStateFilterValue.Disregarded ? palette.Error
                : state == TriStateFilterValue.Included ? palette.AccentPrimary : palette.InputBorder;
            using (Pen pen = new Pen(border, 1.5f)) e.Graphics.DrawRectangle(pen, box);
            if (state == TriStateFilterValue.Included)
            {
                using (SolidBrush fill = new SolidBrush(palette.AccentPrimary)) e.Graphics.FillRectangle(fill, box);
                TextRenderer.DrawText(e.Graphics, "✓", ThemeManager.UiFont(ThemeFontRole.Control, FontStyle.Bold),
                    box, Color.White, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            }
            else if (state == TriStateFilterValue.Disregarded)
            {
                TextRenderer.DrawText(e.Graphics, "×", ThemeManager.UiFont(ThemeFontRole.PanelTitle, FontStyle.Bold),
                    box, palette.Error, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            }
            Rectangle text = new Rectangle(29, 0, Math.Max(1, Width - 31), Height);
            TextRenderer.DrawText(e.Graphics,
                Caption + (state == TriStateFilterValue.Disregarded ? "    Disregarded" : ""),
                Font, text, state == TriStateFilterValue.Disregarded ? palette.Error : palette.TextPrimary,
                TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis);
            if (Focused) ControlPaint.DrawFocusRectangle(e.Graphics, ClientRectangle, palette.BorderFocus, BackColor);
        }

        private void OnStateChanged()
        {
            AccessibleName = Caption + " " + state;
            EventHandler handler = StateChanged;
            if (handler != null) handler(this, EventArgs.Empty);
        }
    }

    internal sealed class ArtworkPreviewSurface : Control
    {
        private Image image;
        public event EventHandler OpenCustomUpscaleRequested;
        public Image Image
        {
            get { return image; }
            set { image = value; Invalidate(); }
        }
        public string EmptyText { get; set; }
        public bool ShowUpscaleAffordance { get; set; }

        public ArtworkPreviewSurface()
        {
            DoubleBuffered = true;
            EmptyText = "No artwork selected";
            Cursor = Cursors.Default;
        }

        protected override void OnMouseMove(MouseEventArgs e)
        {
            Cursor = ShowUpscaleAffordance && UpscaleBounds().Contains(e.Location)
                ? Cursors.Hand : Cursors.Default;
            base.OnMouseMove(e);
        }

        protected override void OnMouseClick(MouseEventArgs e)
        {
            if (ShowUpscaleAffordance && UpscaleBounds().Contains(e.Location))
            {
                EventHandler handler = OpenCustomUpscaleRequested;
                if (handler != null) handler(this, EventArgs.Empty);
            }
            base.OnMouseClick(e);
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            ThemePalette palette = ThemeManager.CurrentPalette;
            e.Graphics.SmoothingMode = SmoothingMode.HighQuality;
            e.Graphics.Clear(palette.TreeSurface);
            Rectangle destination = ImageBounds();
            if (image != null)
                e.Graphics.DrawImage(image, destination);
            else
                TextRenderer.DrawText(e.Graphics, EmptyText, Font, ClientRectangle,
                    palette.TextSecondary, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            if (ShowUpscaleAffordance && image != null)
            {
                Rectangle action = UpscaleBounds();
                using (SolidBrush background = new SolidBrush(Color.FromArgb(220, 11, 31, 54)))
                    e.Graphics.FillRectangle(background, action);
                using (Pen border = new Pen(palette.BorderFocus, 1.5f)) e.Graphics.DrawRectangle(border, action);
                TextRenderer.DrawText(e.Graphics, "☝  Open Custom Upscale", Font, action,
                    palette.TextPrimary, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            }
        }

        private Rectangle ImageBounds()
        {
            if (image == null || Width <= 0 || Height <= 0) return ClientRectangle;
            double scale = Math.Min(Width / (double)image.Width, Height / (double)image.Height);
            int width = Math.Max(1, (int)Math.Round(image.Width * scale));
            int height = Math.Max(1, (int)Math.Round(image.Height * scale));
            return new Rectangle((Width - width) / 2, (Height - height) / 2, width, height);
        }

        private Rectangle UpscaleBounds()
        {
            return new Rectangle(Math.Max(4, Width - 190), 10, 180, 34);
        }
    }
}
