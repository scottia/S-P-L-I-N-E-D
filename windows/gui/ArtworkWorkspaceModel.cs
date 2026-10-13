using System;
using System.Collections.Generic;
using System.Linq;

namespace Splined.WindowsGui
{
    internal enum ArtworkWorkspaceMode
    {
        FindMatch,
        Edit,
        Upscale,
        Advanced,
        MusicBrainz,
        CustomUpscale
    }

    internal enum LibraryNavigationMode
    {
        Artists,
        Albums,
        Folders
    }

    internal enum TriStateFilterValue
    {
        Neutral,
        Included,
        Disregarded
    }

    internal sealed class ArtworkSourceDescriptor
    {
        public string Key;
        public string Label;
        public string Glyph;
        public string Capability;
        public bool BuiltIn;
        public bool Enabled;
        public bool CredentialConfigured;
        public string CredentialStatus = "PENDING";
        public int Priority;
        public int CandidateCount;
        public string Activity = "";

        public ArtworkSourceDescriptor Clone()
        {
            return (ArtworkSourceDescriptor)MemberwiseClone();
        }
    }

    /// <summary>
    /// Presentation metadata for the one authoritative Config v5 source order.
    /// This type never stores a second order; every projection is rebuilt from
    /// ConfigState.Sources and every reorder writes that same list.
    /// </summary>
    internal static class ArtworkSourceCatalog
    {
        private static readonly Dictionary<string, ArtworkSourceDescriptor> Known =
            new Dictionary<string, ArtworkSourceDescriptor>(StringComparer.OrdinalIgnoreCase)
            {
                { "deezer", Source("deezer", "Deezer", "▥", "Artwork search") },
                { "itunes", Source("itunes", "iTunes", "♫", "Apple album artwork") },
                { "fanarttv", Source("fanarttv", "Fanart.tv", "▧", "Community artwork") },
                { "lastfm", Source("lastfm", "Last.fm", "as", "Album artwork") },
                { "musicbrainz", Source("musicbrainz", "MusicBrainz", "⌘", "Release authority + CAA") },
                { "coverartarchive", Source("coverartarchive", "Cover Art Archive", "◇", "Official release artwork") },
                { "discogs", Source("discogs", "Discogs", "◒", "Release database") },
                { "amazon", Source("amazon", "Amazon", "a", "Store artwork") },
                { "navidrome", Source("navidrome", "Navidrome", "◉", "Current server artwork") }
            };

        public static ArtworkSourceDescriptor LocalFiles()
        {
            return new ArtworkSourceDescriptor
            {
                Key = "local",
                Label = "Local Files",
                Glyph = "▰",
                Capability = "Current local artwork",
                BuiltIn = true,
                Enabled = true,
                CredentialConfigured = true,
                Priority = -1
            };
        }

        public static List<ArtworkSourceDescriptor> Project(ConfigState state,
            IDictionary<string, int> candidateCounts)
        {
            List<ArtworkSourceDescriptor> result = new List<ArtworkSourceDescriptor>();
            ArtworkSourceDescriptor local = LocalFiles();
            local.CandidateCount = Count(candidateCounts, local.Key);
            result.Add(local);

            IEnumerable<string> order = (state.Sources ?? new List<string>())
                .Concat(ConfigState.KnownSources.Where(key => state.Sources == null
                    || !state.Sources.Contains(key, StringComparer.OrdinalIgnoreCase)))
                .Distinct(StringComparer.OrdinalIgnoreCase);
            int priority = 0;
            foreach (string key in order)
            {
                ArtworkSourceDescriptor descriptor;
                if (!Known.TryGetValue(key, out descriptor))
                    descriptor = Source(key, DisplayName(key), "●", "Configured artwork source");
                descriptor = descriptor.Clone();
                descriptor.Priority = priority++;
                descriptor.Enabled = !state.ExcludedSources.Contains(key, StringComparer.OrdinalIgnoreCase)
                    && (!state.SourcePolicies.ContainsKey(key) || state.SourcePolicies[key].Enabled);
                descriptor.CredentialStatus = CredentialStore.Status(state, key);
                descriptor.CredentialConfigured = CredentialStore.IsConfiguredStatus(
                    descriptor.CredentialStatus);
                descriptor.CandidateCount = Count(candidateCounts, key);
                result.Add(descriptor);
            }
            return result;
        }

        public static bool Reorder(ConfigState state, string draggedKey, string targetKey)
        {
            if (state == null || String.IsNullOrWhiteSpace(draggedKey)
                || String.IsNullOrWhiteSpace(targetKey)
                || draggedKey.Equals("local", StringComparison.OrdinalIgnoreCase)
                || targetKey.Equals("local", StringComparison.OrdinalIgnoreCase)
                || draggedKey.Equals(targetKey, StringComparison.OrdinalIgnoreCase)) return false;

            List<string> order = state.Sources
                .Concat(ConfigState.KnownSources.Where(key => !state.Sources.Contains(key,
                    StringComparer.OrdinalIgnoreCase)))
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToList();
            int sourceIndex = order.FindIndex(key => key.Equals(draggedKey,
                StringComparison.OrdinalIgnoreCase));
            int targetIndex = order.FindIndex(key => key.Equals(targetKey,
                StringComparison.OrdinalIgnoreCase));
            if (sourceIndex < 0 || targetIndex < 0) return false;
            string value = order[sourceIndex];
            order.RemoveAt(sourceIndex);
            targetIndex = order.FindIndex(key => key.Equals(targetKey,
                StringComparison.OrdinalIgnoreCase));
            order.Insert(Math.Max(0, targetIndex), value);
            state.Sources = order;
            ConfigStore.Save(state);
            return true;
        }

        public static string DisplayName(string key)
        {
            ArtworkSourceDescriptor descriptor;
            if (Known.TryGetValue(key ?? "", out descriptor)) return descriptor.Label;
            if (String.IsNullOrWhiteSpace(key)) return "Unknown";
            return Char.ToUpperInvariant(key[0]) + key.Substring(1);
        }

        private static ArtworkSourceDescriptor Source(string key, string label,
            string glyph, string capability)
        {
            return new ArtworkSourceDescriptor
            {
                Key = key,
                Label = label,
                Glyph = glyph,
                Capability = capability,
                Enabled = true
            };
        }

        private static int Count(IDictionary<string, int> counts, string key)
        {
            int count;
            return counts != null && counts.TryGetValue(key, out count) ? count : 0;
        }

    }

    internal sealed class AdjustmentDefinition
    {
        public string Key;
        public string Label;
        public string Glyph;
        public int Minimum;
        public int Maximum;
        public int SpectrumIndex;
    }

    internal static class AdjustmentCatalog
    {
        public static readonly AdjustmentDefinition[] All =
        {
            Define("picture", "Picture", "▧", -20, 20, 0),
            Define("sharpen", "Sharpen", "△", 0, 20, 1),
            Define("softness", "Softness", "◌", 0, 20, 2),
            Define("contrast", "Contrast", "◐", -20, 20, 3),
            Define("exposure", "Exposure", "☼", -20, 20, 4),
            Define("brightness", "Brightness", "✦", -20, 20, 5),
            Define("gamma", "Gamma", "γ", -20, 20, 6),
            Define("temperature", "Color", "◉", -100, 100, 7)
        };

        public static readonly string[] DefaultPrimary =
            { "picture", "sharpen", "softness", "contrast", "exposure" };

        public static AdjustmentDefinition Find(string key)
        {
            return All.FirstOrDefault(item => item.Key.Equals(key,
                StringComparison.OrdinalIgnoreCase));
        }

        public static List<string> NormalizePrimary(IEnumerable<string> values)
        {
            List<string> result = (values ?? DefaultPrimary)
                .Where(key => Find(key) != null)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Take(5)
                .ToList();
            foreach (string key in DefaultPrimary)
            {
                if (result.Count >= 5) break;
                if (!result.Contains(key, StringComparer.OrdinalIgnoreCase)) result.Add(key);
            }
            return result;
        }

        public static int GetValue(ConfigState state, string key)
        {
            if (key == "picture") return state.UpscalePicturePercent;
            if (key == "sharpen") return state.UpscaleSharpenPercent;
            if (key == "softness") return state.UpscaleSoftnessPercent;
            if (key == "contrast") return state.UpscaleContrastPercent;
            if (key == "exposure") return state.UpscaleExposurePercent;
            if (key == "brightness") return state.UpscaleBrightnessPercent;
            if (key == "gamma") return state.UpscaleGammaPercent;
            return state.UpscaleColorTemperature;
        }

        public static void SetValue(ConfigState state, string key, int value)
        {
            AdjustmentDefinition definition = Find(key);
            if (definition == null) return;
            value = Math.Max(definition.Minimum, Math.Min(definition.Maximum, value));
            if (key == "picture") state.UpscalePicturePercent = value;
            else if (key == "sharpen") state.UpscaleSharpenPercent = value;
            else if (key == "softness") state.UpscaleSoftnessPercent = value;
            else if (key == "contrast") state.UpscaleContrastPercent = value;
            else if (key == "exposure") state.UpscaleExposurePercent = value;
            else if (key == "brightness") state.UpscaleBrightnessPercent = value;
            else if (key == "gamma") state.UpscaleGammaPercent = value;
            else state.UpscaleColorTemperature = value;
        }

        private static AdjustmentDefinition Define(string key, string label,
            string glyph, int minimum, int maximum, int spectrumIndex)
        {
            return new AdjustmentDefinition
            {
                Key = key,
                Label = label,
                Glyph = glyph,
                Minimum = minimum,
                Maximum = maximum,
                SpectrumIndex = spectrumIndex
            };
        }
    }
}
