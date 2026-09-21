using System;
using System.Drawing;
using System.Windows.Forms;

namespace Splined.WindowsGui
{
    internal static class AppIcon
    {
        private static readonly Icon icon = Load();

        public static void Initialize()
        {
            // Touch the cached executable icon before any top-level window is shown.
            Icon unused = icon;
        }

        public static void Apply(Form form)
        {
            if (form == null) return;
            form.ShowIcon = true;
            form.Icon = icon;
        }

        private static Icon Load()
        {
            try
            {
                Icon associated = Icon.ExtractAssociatedIcon(Application.ExecutablePath);
                if (associated != null) return associated;
            }
            catch { }
            return SystemIcons.Application;
        }
    }
}
