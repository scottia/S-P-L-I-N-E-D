using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using System.Web.Script.Serialization;

namespace Splined.WindowsGui
{
    internal sealed class NativeScanRequest
    {
        public string config_text;
        public string path;
        public string indexed_album_path;
        public string indexed_album_key;
        public string compilation_track_path;
        public bool bypass_override;
        public bool review_required = true;
        public bool auto_ideal;
        public string fallback_album;
        public string fallback_artist;
    }

    internal sealed class NativeCoreSession : IDisposable
    {
        private readonly TaskCompletionSource<bool> completion = new TaskCompletionSource<bool>();
        private readonly Action<string, bool> eventSink;
        private readonly NativeCore.EventCallback callback;
        private bool disposed;

        internal NativeCoreSession(string requestJson, Action<string, bool> sink)
        {
            eventSink = sink;
            callback = ReceiveEvent;
            NativeCore.CallStart(requestJson, callback);
        }

        internal Task<bool> Completion { get { return completion.Task; } }

        internal void Submit(string decisionJson)
        {
            NativeCore.CallDecision(decisionJson);
        }

        internal void Cancel()
        {
            NativeCore.CallCancel();
        }

        private void ReceiveEvent(IntPtr bytes, UIntPtr length, IntPtr context)
        {
            try
            {
                string text = NativeCore.DecodeBorrowedUtf8(bytes, length);
                Dictionary<string, object> payload = NativeCore.Serializer.Deserialize<Dictionary<string, object>>(text);
                string name = payload != null && payload.ContainsKey("event")
                    ? Convert.ToString(payload["event"]) : "";
                if (eventSink != null) eventSink("@@SPLINED_GUI@@" + text, false);
                if (String.Equals(name, "scan_idle", StringComparison.Ordinal))
                    completion.TrySetResult(true);
                else if (String.Equals(name, "scan_error", StringComparison.Ordinal))
                    completion.TrySetResult(false);
            }
            catch (Exception error)
            {
                if (eventSink != null) eventSink(error.Message, true);
                completion.TrySetResult(false);
            }
        }

        public void Dispose()
        {
            disposed = true;
            GC.KeepAlive(callback);
        }

        internal bool IsDisposed { get { return disposed; } }
    }

    internal static class NativeCore
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        internal delegate void EventCallback(IntPtr bytes, UIntPtr length, IntPtr context);

        internal static readonly JavaScriptSerializer Serializer = CreateSerializer();
        private static readonly object Gate = new object();
        private static IntPtr libraryHandle;
        private static bool initialized;
        private static ApiVersionCall apiVersionCall;
        private static NoArgumentCall buildIdentityCall;
        private static Utf8Call initializeCall;
        private static Utf8Call mediaSnapshotCall;
        private static Utf8Call embeddedArtworkPreviewCall;
        private static Utf8Call editExistingCoverCall;
        private static StartScanCall startScanCall;
        private static Utf8Call submitDecisionCall;
        private static NoArgumentCall cancelScanCall;
        private static ScanActiveCall scanActiveCall;
        private static FreeStringCall freeStringCall;

        internal static string LibraryPath
        {
            get { return Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "runtime", "splined-core.dll"); }
        }

        internal static bool IsAvailable { get { return File.Exists(LibraryPath); } }

        internal static void Initialize(string applicationRoot, string stateRoot)
        {
            lock (Gate)
            {
                if (initialized) return;
                string path = Path.GetFullPath(LibraryPath);
                if (!File.Exists(path))
                    throw new FileNotFoundException("The fixed SPLINED Rust core DLL was not found.", path);
                libraryHandle = LoadLibraryEx(path, IntPtr.Zero,
                    LoadLibrarySearchDllLoadDir | LoadLibrarySearchSystem32);
                if (libraryHandle == IntPtr.Zero)
                    throw new Win32Exception(Marshal.GetLastWin32Error(),
                        "Windows could not load the fixed SPLINED Rust core DLL: " + path);
                BindExports();
                if (apiVersionCall() != 1)
                    throw new InvalidOperationException("The SPLINED Rust core DLL uses an incompatible native API version.");
                Dictionary<string, object> identity = ReadResponse(buildIdentityCall()) as Dictionary<string, object>;
                if (identity == null
                    || !String.Equals(Convert.ToString(identity["version"]), ReleaseInfo.SemanticVersion,
                        StringComparison.OrdinalIgnoreCase)
                    || (!String.Equals(BuildInfo.Commit, "unknown", StringComparison.OrdinalIgnoreCase)
                        && !String.Equals(Convert.ToString(identity["commit"]), BuildInfo.Commit,
                            StringComparison.OrdinalIgnoreCase)))
                    throw new InvalidOperationException("The SPLINED GUI and Rust core DLL build identities do not match.");
                Dictionary<string, object> request = new Dictionary<string, object>();
                request["application_root"] = Path.GetFullPath(applicationRoot);
                request["state_root"] = Path.GetFullPath(stateRoot);
                CallString(initializeCall, Serializer.Serialize(request));
                initialized = true;
            }
        }

        internal static string MediaSnapshot(ConfigState config, bool refresh)
        {
            Dictionary<string, object> request = new Dictionary<string, object>();
            request["config_text"] = ConfigStore.ExportConfigText(config);
            request["refresh"] = refresh;
            return Serializer.Serialize(CallJson(mediaSnapshotCall, Serializer.Serialize(request)));
        }

        internal static string EmbeddedArtworkPreview(ConfigState config, string trackPath)
        {
            Dictionary<string, object> request = new Dictionary<string, object>();
            request["config_text"] = ConfigStore.ExportConfigText(config);
            request["track_path"] = trackPath;
            object value = CallJson(embeddedArtworkPreviewCall, Serializer.Serialize(request));
            return value == null ? "" : Convert.ToString(value);
        }

        internal static string EditExistingCover(ConfigState config, string path)
        {
            Dictionary<string, object> request = new Dictionary<string, object>();
            request["config_text"] = ConfigStore.ExportConfigText(config);
            request["path"] = path;
            return Serializer.Serialize(CallJson(editExistingCoverCall, Serializer.Serialize(request)));
        }

        internal static NativeCoreSession StartScan(NativeScanRequest request, Action<string, bool> eventSink)
        {
            return new NativeCoreSession(Serializer.Serialize(request), eventSink);
        }

        internal static bool ScanActive { get { return scanActiveCall != null && scanActiveCall(); } }

        internal static void CallStart(string requestJson, EventCallback callback)
        {
            byte[] request = Encoding.UTF8.GetBytes(requestJson);
            IntPtr response = startScanCall(request, (UIntPtr)request.Length, callback, IntPtr.Zero);
            ReadResponse(response);
        }

        internal static void CallDecision(string decisionJson)
        {
            CallString(submitDecisionCall, decisionJson);
        }

        internal static void CallCancel()
        {
            ReadResponse(cancelScanCall());
        }

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr Utf8Call(
            [MarshalAs(UnmanagedType.LPArray, SizeParamIndex = 1, ArraySubType = UnmanagedType.U1)] byte[] request,
            UIntPtr length);

        private static object CallJson(Utf8Call call, string request)
        {
            byte[] bytes = Encoding.UTF8.GetBytes(request);
            return ReadResponse(call(bytes, (UIntPtr)bytes.Length));
        }

        private static void CallString(Utf8Call call, string value)
        {
            CallJson(call, value);
        }

        private static object ReadResponse(IntPtr pointer)
        {
            if (pointer == IntPtr.Zero)
                throw new InvalidOperationException("The SPLINED Rust core returned no native response.");
            string text;
            try
            {
                int length = 0;
                while (Marshal.ReadByte(pointer, length) != 0) length++;
                text = DecodeBorrowedUtf8(pointer, (UIntPtr)length);
            }
            finally { freeStringCall(pointer); }
            Dictionary<string, object> response = Serializer.Deserialize<Dictionary<string, object>>(text);
            if (response == null || !response.ContainsKey("ok"))
                throw new InvalidOperationException("The SPLINED Rust core returned an invalid native response.");
            if (!Convert.ToBoolean(response["ok"]))
                throw new InvalidOperationException(response.ContainsKey("error")
                    ? Convert.ToString(response["error"])
                    : "The SPLINED Rust core reported an unspecified error.");
            return response.ContainsKey("value") ? response["value"] : null;
        }

        internal static string DecodeBorrowedUtf8(IntPtr pointer, UIntPtr length)
        {
            int count = checked((int)length.ToUInt64());
            if (pointer == IntPtr.Zero || count == 0) return "";
            byte[] bytes = new byte[count];
            Marshal.Copy(pointer, bytes, 0, count);
            return Encoding.UTF8.GetString(bytes);
        }

        private static JavaScriptSerializer CreateSerializer()
        {
            JavaScriptSerializer serializer = new JavaScriptSerializer();
            serializer.MaxJsonLength = Int32.MaxValue;
            return serializer;
        }

        private static void BindExports()
        {
            apiVersionCall = Bind<ApiVersionCall>("splined_api_version");
            buildIdentityCall = Bind<NoArgumentCall>("splined_build_identity");
            initializeCall = Bind<Utf8Call>("splined_initialize");
            mediaSnapshotCall = Bind<Utf8Call>("splined_media_snapshot");
            embeddedArtworkPreviewCall = Bind<Utf8Call>("splined_embedded_artwork_preview");
            editExistingCoverCall = Bind<Utf8Call>("splined_edit_existing_cover");
            startScanCall = Bind<StartScanCall>("splined_start_scan");
            submitDecisionCall = Bind<Utf8Call>("splined_submit_decision");
            cancelScanCall = Bind<NoArgumentCall>("splined_cancel_scan");
            scanActiveCall = Bind<ScanActiveCall>("splined_scan_active");
            freeStringCall = Bind<FreeStringCall>("splined_free_string");
        }

        private static T Bind<T>(string name) where T : class
        {
            IntPtr address = GetProcAddress(libraryHandle, name);
            if (address == IntPtr.Zero)
                throw new EntryPointNotFoundException("The SPLINED Rust core DLL is missing native export " + name + ".");
            return (T)(object)Marshal.GetDelegateForFunctionPointer(address, typeof(T));
        }

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate uint ApiVersionCall();

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr NoArgumentCall();

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr StartScanCall(
            [MarshalAs(UnmanagedType.LPArray, SizeParamIndex = 1, ArraySubType = UnmanagedType.U1)] byte[] value,
            UIntPtr length,
            EventCallback callback, IntPtr context);

        [return: MarshalAs(UnmanagedType.I1)]
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate bool ScanActiveCall();

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void FreeStringCall(IntPtr value);

        private const uint LoadLibrarySearchDllLoadDir = 0x00000100;
        private const uint LoadLibrarySearchSystem32 = 0x00000800;

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);

        [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern IntPtr GetProcAddress(IntPtr module, string name);
    }
}
