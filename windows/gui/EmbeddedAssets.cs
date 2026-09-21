using System.Drawing;
using System.IO;
using System.Reflection;

namespace Splined.WindowsGui
{
    internal static class EmbeddedAssets
    {
        private const string ResourcePrefix = "Splined.WindowsGui.Resources.";

        public static Bitmap LoadWatermark()
        {
            return LoadBitmap(ResourcePrefix + "splined-watermark.png");
        }

        public static Bitmap LoadApplicationIconImage()
        {
            return LoadBitmap(ResourcePrefix + "splined-app-icon.png");
        }

        private static Bitmap LoadBitmap(string name)
        {
            Assembly assembly = Assembly.GetExecutingAssembly();
            using (Stream stream = assembly.GetManifestResourceStream(name))
            {
                if (stream == null) return null;
                using (Image source = Image.FromStream(stream))
                    return new Bitmap(source);
            }
        }
    }
}
