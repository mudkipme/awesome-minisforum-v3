// SPDX-License-Identifier: GPL-2.0-only
/*
 * Minisforum V3 (AMI project HPPAC) platform driver.
 *
 * 1. WMI event (always on, passive)
 *    GUID ABBC0F5C-8EA1-11D1-A000-C90629100000, notify 0xD2, raised by EC
 *    query _Q55. Windows "DPMonitor" consumes it as root\WMI DP_EVENT
 *    { Flag = 0 }: a DP-in (display input) change. No payload.
 *    -> sysfs dp_event_count + uevent MINISFORUM_EVENT=DP_IN.
 *    Also keeps huawei-wmi off this shared, generic AMI GUID.
 *
 * 2. EC RAM temperatures (always on, read-only)
 *    Read through the kernel's ACPI EC driver (ec_read), which takes the
 *    proper EC lock. Offsets identified by logging on BIOS 1.05.
 *
 * 3. Vendor EC command channel (module param enable_ec_cmds, default on)
 *    Ports 0x6C (cmd/status: bit1 IBF, bit0 OBF) and 0x68 (data), as used
 *    by Minisforum Space's minisforum.dll and by the BIOS itself (Setup
 *    "EC IO Hardware Monitor" page, OemDxe). Only these are ever sent
 *    (hard whitelist below):
 *      0xD5 0x18 / 0x19       CPU fan speed lo / hi (RPM)     [DLL + BIOS]
 *      0xD5 0x16 / 0x17       System fan speed lo / hi (RPM)  [BIOS]
 *      0xDD 0x20              CPU temperature (degC)          [DLL + BIOS]
 *      0xDD 0x21              System temperature (degC)       [BIOS]
 *      0xDE 0x04              read thermal mode -> 0xB0/0xB1/0xB2 [DLL + OemDxe]
 *      0xDE 1|2|3             set thermal mode (only if allow_profile_set=1) [DLL]
 *    Handshake and pacing follow the BIOS: wait IBF clear, cmd, wait, arg,
 *    wait OBF, read; ~3 ms gap between commands.
 *    The mode is mirrored by the BIOS in CMOS 0x63 and the Setup "FAN Mode"
 *    option (Quiet/Balance/Performance), so it can persist across reboots.
 *
 * 4. platform_profile (low-power / balanced / performance)
 *    Reading always works; changing it requires allow_profile_set=1.
 *    Like the vendor app, performance is refused when not on AC power.
 */

#include <linux/acpi.h>
#include <linux/atomic.h>
#include <linux/delay.h>
#include <linux/device.h>
#include <linux/dmi.h>
#include <linux/hwmon.h>
#include <linux/io.h>
#include <linux/ioport.h>
#include <linux/jiffies.h>
#include <linux/kobject.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/platform_profile.h>
#include <linux/power_supply.h>
#include <linux/sysfs.h>
#include <linux/wmi.h>

#define V3_EVENT_GUID	"ABBC0F5C-8EA1-11D1-A000-C90629100000"

#define V3_EC_DATA	0x68
#define V3_EC_CMD	0x6C
#define V3_EC_OBF	BIT(0)
#define V3_EC_IBF	BIT(1)

#define V3_CMD_FAN	0xD5
#define V3_ARG_FAN1_LO	0x18	/* CPU fan */
#define V3_ARG_FAN1_HI	0x19
#define V3_ARG_FAN2_LO	0x16	/* System fan */
#define V3_ARG_FAN2_HI	0x17
#define V3_CMD_TEMP	0xDD
#define V3_ARG_CPUTEMP	0x20
#define V3_ARG_SYSTEMP	0x21
#define V3_CMD_MODE	0xDE
#define V3_ARG_MODE_GET	0x04

enum v3_mode { V3_MODE_SAVING = 1, V3_MODE_BALANCE = 2, V3_MODE_PERF = 3 };

static bool enable_ec_cmds = true;
module_param(enable_ec_cmds, bool, 0444);
MODULE_PARM_DESC(enable_ec_cmds, "Use the vendor EC command channel 0x68/0x6C for fan RPM, CPU temp and thermal mode (default: on)");

static bool force_unknown_fw;
module_param(force_unknown_fw, bool, 0444);
MODULE_PARM_DESC(force_unknown_fw, "Use the vendor EC channel even on BIOS/EC versions that were not verified (default: off)");

/*
 * Firmware the vendor EC commands were verified against:
 * BIOS 1.05 (running, hardware-tested) and 1.06 (image analysed: only adds an
 * SPD entry, same EC 8.08). Anything else keeps only the passive features.
 */
static const char * const v3_known_bios[] = { "1.05", "1.06" };
static const char * const v3_known_ec[] = { "8.8" };

static bool v3_str_in(const char *s, const char * const *list, size_t n)
{
	size_t i;

	for (i = 0; s && i < n; i++)
		if (!strcmp(s, list[i]))
			return true;
	return false;
}

static bool v3_fw_known(struct device *dev)
{
	const char *bios = dmi_get_system_info(DMI_BIOS_VERSION);
	const char *ec = dmi_get_system_info(DMI_EC_FIRMWARE_RELEASE);
	bool ok = v3_str_in(bios, v3_known_bios, ARRAY_SIZE(v3_known_bios)) &&
		  v3_str_in(ec, v3_known_ec, ARRAY_SIZE(v3_known_ec));

	dev_info(dev, "BIOS %s, EC %s: %s\n", bios ?: "?", ec ?: "?",
		 ok ? "verified firmware" : "UNVERIFIED firmware");
	return ok;
}

static bool allow_profile_set;
module_param(allow_profile_set, bool, 0644);
MODULE_PARM_DESC(allow_profile_set, "Allow changing the EC thermal mode via platform_profile (default: off = read-only)");

/* EC RAM temperature sensors (degC, one byte each), ACPI EC address space */
static const struct {
	u8 off;
	const char *label;
} v3_ec_temps[] = {
	{ 0x09, "APU die" },		/* DIEH in DSDT */
	{ 0x04, "SoC sensor A" },	/* tracks APU, r=0.79 */
	{ 0x07, "SoC sensor B" },	/* tracks APU, r=0.83 */
	{ 0x06, "Board" },		/* slow, partly tracks APU */
	{ 0x05, "System" },		/* = EC cmd 0xDD 0x21; UM780 uses 0x05 too */
	{ 0x08, "Ambient?" },		/* ~flat, coolest sensor */
};
#define V3_N_EC_TEMPS ARRAY_SIZE(v3_ec_temps)
/* hwmon temp channels: [0..N-1] EC RAM sensors, [N] EC-cmd CPU, [N+1] EC-cmd System */
#define V3_CH_CMD_CPU	V3_N_EC_TEMPS
#define V3_CH_CMD_SYS	(V3_N_EC_TEMPS + 1)

struct v3_priv {
	struct device *dev;
	atomic_t dp_events;
	bool ec_cmds;			/* ports claimed and enabled */
	struct mutex ec_lock;		/* serialises 0x68/0x6C transactions */
	unsigned long fan_stamp[2];	/* jiffies of last fan read */
	int fan_rpm[2];
	int user_mode;			/* last mode set through us, 0 = none */
};

static const struct dmi_system_id v3_dmi_table[] = {
	{
		.ident = "Minisforum V3",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "Micro Computer (HK) Tech Limited"),
			DMI_MATCH(DMI_PRODUCT_NAME, "V3"),
			DMI_MATCH(DMI_BOARD_NAME, "HPPAC"),
		},
	},
	{ }
};

/* ---------------- vendor EC channel ---------------- */

static int v3_wait(u8 mask, bool want_set)
{
	int i;

	/* vendor: IBF 10 x 100 ms, OBF 10 x 15 ms; we poll finer, ~200 ms max */
	for (i = 0; i < 400; i++) {
		bool set = inb(V3_EC_CMD) & mask;

		if (set == want_set)
			return 0;
		usleep_range(400, 600);
	}
	return -ETIMEDOUT;
}

static bool v3_cmd_allowed(u8 cmd, u8 arg, bool expects_reply)
{
	switch (cmd) {
	case V3_CMD_FAN:
		return expects_reply && arg >= V3_ARG_FAN2_LO && arg <= V3_ARG_FAN1_HI;
	case V3_CMD_TEMP:
		return expects_reply && (arg == V3_ARG_CPUTEMP || arg == V3_ARG_SYSTEMP);
	case V3_CMD_MODE:
		if (expects_reply)
			return arg == V3_ARG_MODE_GET;
		return arg >= V3_MODE_SAVING && arg <= V3_MODE_PERF;
	}
	return false;
}

/* cmd -> 0x6C, arg -> 0x68, optional reply <- 0x68. Caller holds ec_lock. */
static int v3_ec_xfer(struct v3_priv *p, u8 cmd, u8 arg, u8 *reply)
{
	int ret;

	if (!p->ec_cmds)
		return -EOPNOTSUPP;
	if (WARN_ON_ONCE(!v3_cmd_allowed(cmd, arg, reply)))
		return -EPERM;

	lockdep_assert_held(&p->ec_lock);

	/* drop a stale byte so it can't be mistaken for our reply */
	if (inb(V3_EC_CMD) & V3_EC_OBF)
		inb(V3_EC_DATA);

	ret = v3_wait(V3_EC_IBF, false);
	if (ret)
		goto out;
	outb(cmd, V3_EC_CMD);
	ret = v3_wait(V3_EC_IBF, false);
	if (ret)
		goto out;
	outb(arg, V3_EC_DATA);

	if (reply) {
		ret = v3_wait(V3_EC_OBF, true);
		if (ret)
			goto out;
		*reply = inb(V3_EC_DATA);
	} else {
		ret = v3_wait(V3_EC_IBF, false);	/* EC consumed it */
	}
out:
	if (ret)
		dev_warn_ratelimited(p->dev, "EC cmd %#04x/%#04x timed out\n", cmd, arg);
	/* the BIOS spaces EC commands by ~3 ms (3000 port-0x80 reads) */
	usleep_range(3000, 3500);
	return ret;
}

/* hi, lo, hi again; retry if the high byte moved (torn read), as minisforum-um780xtx */
static int v3_read_fan_raw(struct v3_priv *p, int fan, int *rpm)
{
	u8 lo_arg = fan ? V3_ARG_FAN2_LO : V3_ARG_FAN1_LO;
	u8 hi_arg = fan ? V3_ARG_FAN2_HI : V3_ARG_FAN1_HI;
	u8 hi, lo, hi2;
	int ret, i;

	for (i = 0; i < 3; i++) {
		ret = v3_ec_xfer(p, V3_CMD_FAN, hi_arg, &hi);
		if (!ret)
			ret = v3_ec_xfer(p, V3_CMD_FAN, lo_arg, &lo);
		if (!ret)
			ret = v3_ec_xfer(p, V3_CMD_FAN, hi_arg, &hi2);
		if (ret)
			return ret;
		if (hi == hi2) {
			*rpm = hi << 8 | lo;
			/* the BIOS shows N/A for 0xFFFF */
			return *rpm == 0xFFFF ? -ENODATA : 0;
		}
	}
	return -EAGAIN;
}

static int v3_read_fan(struct v3_priv *p, int fan, long *rpm)
{
	int ret = 0;

	mutex_lock(&p->ec_lock);
	/* cache 1 s: hwmon readers poll; the vendor app polls every 5 s */
	if (!p->fan_stamp[fan] || time_after(jiffies, p->fan_stamp[fan] + HZ)) {
		ret = v3_read_fan_raw(p, fan, &p->fan_rpm[fan]);
		if (!ret)
			p->fan_stamp[fan] = jiffies;
	}
	*rpm = p->fan_rpm[fan];
	mutex_unlock(&p->ec_lock);
	return ret;
}

static int v3_read_cmdtemp(struct v3_priv *p, u8 arg, long *mdeg)
{
	u8 t;
	int ret;

	mutex_lock(&p->ec_lock);
	ret = v3_ec_xfer(p, V3_CMD_TEMP, arg, &t);
	mutex_unlock(&p->ec_lock);
	if (!ret)
		*mdeg = t * 1000L;
	return ret;
}

static int v3_get_mode(struct v3_priv *p, enum v3_mode *mode)
{
	u8 r;
	int ret;

	mutex_lock(&p->ec_lock);
	ret = v3_ec_xfer(p, V3_CMD_MODE, V3_ARG_MODE_GET, &r);
	mutex_unlock(&p->ec_lock);
	if (ret)
		return ret;

	switch (r) {
	case 0xB0: *mode = V3_MODE_SAVING;  return 0;
	case 0xB1: *mode = V3_MODE_BALANCE; return 0;
	case 0xB2: *mode = V3_MODE_PERF;    return 0;
	}
	dev_warn_ratelimited(p->dev, "unexpected thermal mode byte %#04x\n", r);
	return -EIO;
}

static int v3_set_mode(struct v3_priv *p, enum v3_mode mode)
{
	int ret;

	mutex_lock(&p->ec_lock);
	ret = v3_ec_xfer(p, V3_CMD_MODE, mode, NULL);
	if (!ret)
		p->user_mode = mode;
	mutex_unlock(&p->ec_lock);
	return ret;
}

/* ---------------- platform_profile ---------------- */

static int v3_pp_probe(void *drvdata, unsigned long *choices)
{
	set_bit(PLATFORM_PROFILE_LOW_POWER, choices);
	set_bit(PLATFORM_PROFILE_BALANCED, choices);
	set_bit(PLATFORM_PROFILE_PERFORMANCE, choices);
	return 0;
}

static int v3_pp_get(struct device *dev, enum platform_profile_option *profile)
{
	struct v3_priv *p = dev_get_drvdata(dev);
	enum v3_mode m;
	int ret = v3_get_mode(p, &m);

	if (ret)
		return ret;
	*profile = m == V3_MODE_SAVING ? PLATFORM_PROFILE_LOW_POWER :
		   m == V3_MODE_PERF   ? PLATFORM_PROFILE_PERFORMANCE :
					 PLATFORM_PROFILE_BALANCED;
	return 0;
}

static int v3_pp_set(struct device *dev, enum platform_profile_option profile)
{
	struct v3_priv *p = dev_get_drvdata(dev);
	enum v3_mode m;

	if (!allow_profile_set) {
		dev_notice_ratelimited(p->dev, "profile change ignored: load with allow_profile_set=1\n");
		return -EPERM;
	}

	switch (profile) {
	case PLATFORM_PROFILE_LOW_POWER:   m = V3_MODE_SAVING;  break;
	case PLATFORM_PROFILE_BALANCED:    m = V3_MODE_BALANCE; break;
	case PLATFORM_PROFILE_PERFORMANCE:
		/* same rule as Minisforum Space: no performance on battery */
		if (!power_supply_is_system_supplied())
			return -EBUSY;
		m = V3_MODE_PERF;
		break;
	default:
		return -EOPNOTSUPP;
	}
	return v3_set_mode(p, m);
}

static const struct platform_profile_ops v3_pp_ops = {
	.probe = v3_pp_probe,
	.profile_get = v3_pp_get,
	.profile_set = v3_pp_set,
};

/* ---------------- hwmon ---------------- */

static umode_t v3_hwmon_visible(const void *data, enum hwmon_sensor_types type,
				u32 attr, int ch)
{
	const struct v3_priv *p = data;

	if (type == hwmon_fan)
		return p->ec_cmds ? 0444 : 0;
	if (type == hwmon_temp && ch >= V3_CH_CMD_CPU)
		return p->ec_cmds ? 0444 : 0;
	return 0444;
}

static int v3_hwmon_read(struct device *dev, enum hwmon_sensor_types type,
			 u32 attr, int ch, long *val)
{
	struct v3_priv *p = dev_get_drvdata(dev);
	u8 v;
	int ret;

	if (type == hwmon_fan)
		return v3_read_fan(p, ch, val);

	if (ch == V3_CH_CMD_CPU)
		return v3_read_cmdtemp(p, V3_ARG_CPUTEMP, val);
	if (ch == V3_CH_CMD_SYS)
		return v3_read_cmdtemp(p, V3_ARG_SYSTEMP, val);

	ret = ec_read(v3_ec_temps[ch].off, &v);
	if (ret)
		return ret;
	*val = v * 1000L;
	return 0;
}

static int v3_hwmon_read_string(struct device *dev, enum hwmon_sensor_types type,
				u32 attr, int ch, const char **str)
{
	if (type == hwmon_fan)
		*str = ch ? "System Fan" : "CPU Fan";
	else if (ch == V3_CH_CMD_CPU)
		*str = "CPU (EC cmd)";
	else if (ch == V3_CH_CMD_SYS)
		*str = "System (EC cmd)";
	else
		*str = v3_ec_temps[ch].label;
	return 0;
}

static const struct hwmon_ops v3_hwmon_ops = {
	.is_visible = v3_hwmon_visible,
	.read = v3_hwmon_read,
	.read_string = v3_hwmon_read_string,
};

static const struct hwmon_channel_info * const v3_hwmon_info[] = {
	HWMON_CHANNEL_INFO(fan, HWMON_F_INPUT | HWMON_F_LABEL, HWMON_F_INPUT | HWMON_F_LABEL),
	HWMON_CHANNEL_INFO(temp,
			   HWMON_T_INPUT | HWMON_T_LABEL, HWMON_T_INPUT | HWMON_T_LABEL,
			   HWMON_T_INPUT | HWMON_T_LABEL, HWMON_T_INPUT | HWMON_T_LABEL,
			   HWMON_T_INPUT | HWMON_T_LABEL, HWMON_T_INPUT | HWMON_T_LABEL,
			   HWMON_T_INPUT | HWMON_T_LABEL,
			   HWMON_T_INPUT | HWMON_T_LABEL),	/* 6 EC RAM + 2 EC cmd */
	NULL
};

static const struct hwmon_chip_info v3_hwmon_chip = {
	.ops = &v3_hwmon_ops,
	.info = v3_hwmon_info,
};

/* ---------------- WMI event + sysfs ---------------- */

static ssize_t dp_event_count_show(struct device *dev,
				   struct device_attribute *attr, char *buf)
{
	struct v3_priv *p = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%d\n", atomic_read(&p->dp_events));
}
static DEVICE_ATTR_RO(dp_event_count);

static struct attribute *v3_wmi_attrs[] = {
	&dev_attr_dp_event_count.attr,
	NULL
};
ATTRIBUTE_GROUPS(v3_wmi);

static void v3_wmi_notify(struct wmi_device *wdev, const struct wmi_buffer *data)
{
	struct v3_priv *p = dev_get_drvdata(&wdev->dev);
	char *envp[] = { "MINISFORUM_EVENT=DP_IN", NULL };

	atomic_inc(&p->dp_events);
	dev_dbg(&wdev->dev, "DP-in event (%zu byte payload)\n", data ? data->length : 0);
	kobject_uevent_env(&wdev->dev.kobj, KOBJ_CHANGE, envp);
}

static void v3_release_ports(void *unused)
{
	release_region(V3_EC_DATA, 1);
	release_region(V3_EC_CMD, 1);
}

static bool v3_claim_ports(struct device *dev)
{
	if (!request_region(V3_EC_DATA, 1, "minisforum-v3-ec"))
		return false;
	if (!request_region(V3_EC_CMD, 1, "minisforum-v3-ec")) {
		release_region(V3_EC_DATA, 1);
		return false;
	}
	return !devm_add_action_or_reset(dev, v3_release_ports, NULL);
}

static int v3_wmi_probe(struct wmi_device *wdev, const void *context)
{
	struct device *dev = &wdev->dev;
	struct v3_priv *p;
	struct device *hw, *pp;
	enum v3_mode m;

	/* The GUID is a generic AMI sample GUID used by other vendors too. */
	if (!dmi_check_system(v3_dmi_table))
		return -ENODEV;

	p = devm_kzalloc(dev, sizeof(*p), GFP_KERNEL);
	if (!p)
		return -ENOMEM;
	p->dev = dev;
	atomic_set(&p->dp_events, 0);
	if (devm_mutex_init(dev, &p->ec_lock))
		return -ENOMEM;
	dev_set_drvdata(dev, p);

	if (enable_ec_cmds && !v3_fw_known(dev) && !force_unknown_fw) {
		dev_warn(dev, "vendor EC commands disabled on unverified firmware (force_unknown_fw=1 to override)\n");
	} else if (enable_ec_cmds) {
		if (v3_claim_ports(dev)) {
			p->ec_cmds = true;
			/* sanity check: the channel must answer with a valid mode */
			if (v3_get_mode(p, &m)) {
				dev_warn(dev, "EC channel 0x68/0x6C not responding; disabling it\n");
				p->ec_cmds = false;
			}
		} else {
			dev_warn(dev, "ports 0x68/0x6C busy; EC commands disabled\n");
		}
	}

	hw = devm_hwmon_device_register_with_info(dev, "minisforum_v3", p,
						  &v3_hwmon_chip, NULL);
	if (IS_ERR(hw))
		dev_warn(dev, "hwmon registration failed: %ld\n", PTR_ERR(hw));

	if (p->ec_cmds) {
		pp = devm_platform_profile_register(dev, "minisforum-v3", p, &v3_pp_ops);
		if (IS_ERR(pp))
			dev_warn(dev, "platform_profile registration failed: %ld\n", PTR_ERR(pp));
	}

	dev_info(dev, "Minisforum V3: DP-in events, EC temps%s%s\n",
		 p->ec_cmds ? ", fan/CPU temp/thermal mode" : "",
		 p->ec_cmds && !allow_profile_set ? " (profile read-only)" : "");
	return 0;
}

/* s2idle can reset EC state on Minisforum ECs (see minisforum-um780xtx) */
static int v3_resume(struct device *dev)
{
	struct v3_priv *p = dev_get_drvdata(dev);
	enum v3_mode cur;

	if (!p->ec_cmds || !p->user_mode || !allow_profile_set)
		return 0;
	if (p->user_mode == V3_MODE_PERF && !power_supply_is_system_supplied())
		return 0;
	if (!v3_get_mode(p, &cur) && cur != p->user_mode) {
		dev_info(dev, "restoring thermal mode %d after resume\n", p->user_mode);
		v3_set_mode(p, p->user_mode);
	}
	return 0;
}
static DEFINE_SIMPLE_DEV_PM_OPS(v3_pm_ops, NULL, v3_resume);

static const struct wmi_device_id v3_wmi_id_table[] = {
	{ .guid_string = V3_EVENT_GUID },
	{ }
};
MODULE_DEVICE_TABLE(wmi, v3_wmi_id_table);

static struct wmi_driver v3_wmi_driver = {
	.driver = {
		.name = "minisforum-v3-wmi",
		.dev_groups = v3_wmi_groups,
		.pm = pm_sleep_ptr(&v3_pm_ops),
		.probe_type = PROBE_PREFER_ASYNCHRONOUS,
	},
	.id_table = v3_wmi_id_table,
	.min_event_size = 0,	/* event has no payload */
	.probe = v3_wmi_probe,
	.notify_new = v3_wmi_notify,
};
module_wmi_driver(v3_wmi_driver);

MODULE_DESCRIPTION("Minisforum V3 platform driver (WMI events, hwmon, platform_profile)");
MODULE_LICENSE("GPL");
