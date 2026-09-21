package org.emergencysim.alertinject;

import android.content.Context;
import android.content.Intent;
import android.os.Looper;

import java.lang.reflect.Constructor;
import java.lang.reflect.Method;

/**
 * Development-only Cell Broadcast test injector.
 *
 * <p>This tool exists solely to drive the genuine Android emergency-alert pipeline on a
 * developer-owned test device, so that the Emergency Simulator project can verify the real
 * {@code CellBroadcastReceiver} path. It is not a consumer-facing component.
 *
 * <p>Safety properties, enforced in code rather than by convention:
 * <ul>
 *   <li>The alert type is pinned to the ETWS <em>test message</em> warning type (0x03). There is no
 *       option to select a real hazard category.
 *   <li>The body text must begin with {@code TEST}, or the tool refuses to send.
 *   <li>Nothing is sent automatically. There is no scheduling and no retry.
 *   <li>The target package is supplied by the controller from its on-device Cell Broadcast package discovery and is validated before use.
 * </ul>
 *
 * <p>It transmits nothing. There is no radio, modem or network participation of any kind: the
 * message is injected locally into the Cell Broadcast receiver on this device. It cannot reach, and
 * does not attempt to reach, any cellular network or any other device.
 *
 * <p>Invocation, on a device the developer controls:
 * <pre>
 *   adb root
 *   adb shell CLASSPATH=/data/local/tmp/alertinject.jar app_process /system/bin \
 *       org.emergencysim.alertinject.AlertInjector 4355 com.google.android.cellbroadcastreceiver "TEST ALERT - SIMULATION"
 * </pre>
 */
public final class AlertInjector {

    /** Note: "provider.action", not "provider.Telephony". A wrong string is a silent no-op. */
    private static final String ACTION_EMERGENCY =
            "android.provider.action.SMS_EMERGENCY_CB_RECEIVED";

    /** The extra key CellBroadcastAlertService reads. Verified against android15-release source. */
    private static final String EXTRA_MESSAGE = "message";

    /** SmsCbMessage.MESSAGE_FORMAT_3GPP */
    private static final int MESSAGE_FORMAT_3GPP = 1;

    /** SmsCbMessage.GEOGRAPHICAL_SCOPE_CELL_WIDE_IMMEDIATE */
    private static final int SCOPE_CELL_WIDE_IMMEDIATE = 0;

    /** SmsCbEtwsInfo.ETWS_WARNING_TYPE_TEST_MESSAGE - the only warning type this tool will send. */
    private static final int ETWS_WARNING_TYPE_TEST_MESSAGE = 0x03;

    private AlertInjector() {
    }

    public static void main(String[] args) {
        if (args.length < 2) {
            System.err.println("usage: AlertInjector <serviceCategory> <cellBroadcastPackage> [body]");
            System.err.println("  the body must begin with TEST (this is enforced)");
            System.exit(2);
        }

        int serviceCategory;
        String targetPackage = args[1];
        if (!isAllowedTargetPackage(targetPackage)) {
            System.err.println("FATAL: refusing invalid CellBroadcast target package: " + targetPackage);
            System.exit(4);
            return;
        }

        try {
            serviceCategory = Integer.parseInt(args[0]);
        } catch (NumberFormatException e) {
            System.err.println("FATAL: serviceCategory must be an integer, got: " + args[0]);
            System.exit(2);
            return;
        }

        String body = args.length > 2
                ? args[2]
                : "TEST ALERT - SIMULATION. Emergency Simulator development harness.";

        if (!body.startsWith("TEST")) {
            System.err.println("FATAL: refusing to send. The body must begin with TEST so the "
                    + "alert is unambiguously identifiable as a test. Got: " + body);
            System.exit(3);
            return;
        }

        try {
            int subId = 0;

            System.out.println("AlertInjector: building SmsCbMessage");
            System.out.println("  serviceCategory = " + serviceCategory);
            System.out.println("  warningType     = " + ETWS_WARNING_TYPE_TEST_MESSAGE
                    + " (ETWS TEST MESSAGE)");
            System.out.println("  targetPackage   = " + targetPackage);
            System.out.println("  subId           = " + subId);
            System.out.println("  body            = " + body);

            Object message = buildSmsCbMessage(serviceCategory, body, subId);

            Context context = systemContext();
            System.out.println("  context         = " + context);

            Intent intent = new Intent(ACTION_EMERGENCY);
            intent.setPackage(targetPackage);
            intent.putExtra(EXTRA_MESSAGE, (android.os.Parcelable) message);

            context.sendBroadcast(intent);
            System.out.println("AlertInjector: broadcast sent to " + targetPackage);

            // Give the receiver a moment to log before this process exits.
            Thread.sleep(3000L);
            System.out.println("AlertInjector: done");
            System.exit(0);
        } catch (Throwable t) {
            System.err.println("FATAL: " + t);
            t.printStackTrace(System.err);
            System.exit(1);
        }
    }

    private static boolean isAllowedTargetPackage(String packageName) {
        if (packageName == null || packageName.isEmpty() || packageName.length() > 128) {
            return false;
        }
        for (int i = 0; i < packageName.length(); i++) {
            char c = packageName.charAt(i);
            if (!(Character.isLetterOrDigit(c) || c == '.' || c == '_')) {
                return false;
            }
        }
        return packageName.toLowerCase(java.util.Locale.ROOT).contains("cellbroadcast");
    }

    /**
     * Builds the message reflectively. {@code SmsCbMessage} and its companions are hidden from the
     * public SDK, so they cannot be referenced at compile time; at runtime they are present in the
     * boot classpath.
     *
     * <p>Signature taken from {@code frameworks/base/telephony/java/android/telephony/SmsCbMessage.java}
     * on {@code android15-release}:
     * <pre>
     * SmsCbMessage(int messageFormat, int geographicalScope, int serialNumber, SmsCbLocation location,
     *              int serviceCategory, String language, String body, int priority,
     *              SmsCbEtwsInfo etwsWarningInfo, SmsCbCmasInfo cmasWarningInfo, int slotIndex,
     *              int subId)
     * </pre>
     */
    private static Object buildSmsCbMessage(int serviceCategory, String body, int subId)
            throws Exception {
        Class<?> locationClass = Class.forName("android.telephony.SmsCbLocation");
        Class<?> etwsClass = Class.forName("android.telephony.SmsCbEtwsInfo");
        Class<?> cmasClass = Class.forName("android.telephony.SmsCbCmasInfo");
        Class<?> messageClass = Class.forName("android.telephony.SmsCbMessage");

        Constructor<?> locationCtor =
                locationClass.getDeclaredConstructor(String.class, int.class, int.class);
        locationCtor.setAccessible(true);
        Object location = locationCtor.newInstance("00101", -1, -1);

        Constructor<?> etwsCtor = etwsClass.getDeclaredConstructor(
                int.class, boolean.class, boolean.class, boolean.class, byte[].class);
        etwsCtor.setAccessible(true);
        Object etwsInfo = etwsCtor.newInstance(
                ETWS_WARNING_TYPE_TEST_MESSAGE,
                true,   // isEmergencyUserAlert
                true,   // isPopupAlert -> request the popup / full-screen presentation
                true,   // isPrimary
                null);  // warningSecurityInformation

        Constructor<?> messageCtor = messageClass.getDeclaredConstructor(
                int.class, int.class, int.class, locationClass, int.class,
                String.class, String.class, int.class, etwsClass, cmasClass,
                int.class, int.class);
        messageCtor.setAccessible(true);

        return messageCtor.newInstance(
                MESSAGE_FORMAT_3GPP,
                SCOPE_CELL_WIDE_IMMEDIATE,
                1,              // serialNumber
                location,
                serviceCategory,
                "en",           // language
                body,
                0,              // priority
                etwsInfo,
                null,           // cmasWarningInfo - null: this is an ETWS test, not a CMAS alert
                0,              // slotIndex
                subId);
    }

    /**
     * Obtains a Context without running as an installed application. {@code AlertInjector} is
     * launched through {@code app_process} rather than as an APK, so it has no Application object.
     * {@code ActivityThread} is hidden, so it is reached reflectively.
     */
    private static Context systemContext() throws Exception {
        if (Looper.myLooper() == null) {
            Looper.prepareMainLooper();
        }
        Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
        try {
            // Modern API: no-argument overload.
            Method systemMain = activityThreadClass.getMethod("systemMain");
            Object activityThread = systemMain.invoke(null);
            Method getSystemContext = activityThreadClass.getMethod("getSystemContext");
            return (Context) getSystemContext.invoke(activityThread);
        } catch (NoSuchMethodException olderApi) {
            // Older API: systemMain(Thread) / currentActivityThread().
            Method current = activityThreadClass.getMethod("currentActivityThread");
            Object activityThread = current.invoke(null);
            Method getSystemContext = activityThreadClass.getMethod("getSystemContext");
            return (Context) getSystemContext.invoke(activityThread);
        }
    }
}