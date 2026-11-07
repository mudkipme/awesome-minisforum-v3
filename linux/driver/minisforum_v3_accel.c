// SPDX-License-Identifier: GPL-2.0-only
/*
 * Minisforum V3: bind the in-kernel st_lsm6dsx driver to the ACPI SMOCF05 IMU.
 *
 * The firmware describes an ST LSM6DS3TR-C (accel + gyro) at I2C 0x6A as
 * ACPI "SMOCF05". Mainline st_lsm6dsx only matches SMO8B30/SMOCF00 (patch
 * "iio: imu: lsm6dsx: Support SMOCF05 ACPI ID" v2, Feb 2026, not merged), so
 * nothing binds and there is no auto-rotate.
 *
 * This shim replaces the unbound ACPI I2C client with an identical one named
 * "lsm6ds3tr-c", carrying the firmware's accelerometer mount matrix (SLA0)
 * as a "mount-matrix" property and the ACPI GpioInt as IRQ. The stock
 * st_lsm6dsx_i2c then probes it and verifies WHO_AM_I itself (0x6A for
 * LSM6DS3TR-C); if the chip were different, its probe simply fails.
 *
 * Unloading removes our client. The original ACPI client returns on reboot.
 */

#include <linux/acpi.h>
#include <linux/dmi.h>
#include <linux/i2c.h>
#include <linux/module.h>
#include <linux/property.h>

#define V3_IMU_HID	"SMOCF05"
#define V3_IMU_NAME	"lsm6ds3tr-c"

static bool firmware_matrix = true;
module_param(firmware_matrix, bool, 0444);
MODULE_PARM_DESC(firmware_matrix, "Use the DSDT SLA0 accel matrix diag(-1,-1,-1) (default) instead of identity");

static const struct dmi_system_id v3_dmi_table[] = {
	{
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "Micro Computer (HK) Tech Limited"),
			DMI_MATCH(DMI_PRODUCT_NAME, "V3"),
		},
	},
	{ }
};

/* DSDT \_SB.I2CD.STS.SLA0: "-1 0 0", "0 -1 0", "0 0 -1" */
static const char * const v3_matrix_fw[] = {
	"-1", "0", "0",
	"0", "-1", "0",
	"0", "0", "-1",
};
static const char * const v3_matrix_id[] = {
	"1", "0", "0",
	"0", "1", "0",
	"0", "0", "1",
};

static struct property_entry v3_props[2];
static const struct software_node v3_swnode = {
	.name = "minisforum-v3-imu",
	.properties = v3_props,
};

static struct i2c_client *v3_client;

static int __init v3_accel_init(void)
{
	struct i2c_board_info info = { };
	struct acpi_device *adev;
	struct i2c_client *orig;
	struct i2c_adapter *adap;
	int irq;

	if (!dmi_check_system(v3_dmi_table))
		return -ENODEV;

	adev = acpi_dev_get_first_match_dev(V3_IMU_HID, NULL, -1);
	if (!adev)
		return -ENODEV;

	orig = i2c_find_device_by_fwnode(acpi_fwnode_handle(adev));
	if (!orig) {
		acpi_dev_put(adev);
		pr_info("minisforum_v3_accel: no %s I2C client\n", V3_IMU_HID);
		return -ENODEV;
	}

	if (orig->dev.driver) {
		/* a kernel with the upstream SMOCF05 patch already handles it */
		pr_info("minisforum_v3_accel: %s already bound to %s, nothing to do\n",
			V3_IMU_HID, orig->dev.driver->name);
		put_device(&orig->dev);
		acpi_dev_put(adev);
		return -EEXIST;
	}

	irq = acpi_dev_gpio_irq_get(adev, 0);
	acpi_dev_put(adev);

	adap = orig->adapter;
	get_device(&adap->dev);

	if (firmware_matrix)
		v3_props[0] = PROPERTY_ENTRY_STRING_ARRAY("mount-matrix", v3_matrix_fw);
	else
		v3_props[0] = PROPERTY_ENTRY_STRING_ARRAY("mount-matrix", v3_matrix_id);

	strscpy(info.type, V3_IMU_NAME, sizeof(info.type));
	info.addr = orig->addr;
	info.irq = irq > 0 ? irq : 0;
	info.swnode = &v3_swnode;

	i2c_unregister_device(orig);
	put_device(&orig->dev);

	v3_client = i2c_new_client_device(adap, &info);
	put_device(&adap->dev);
	if (IS_ERR(v3_client)) {
		pr_err("minisforum_v3_accel: creating %s client failed: %ld\n",
		       V3_IMU_NAME, PTR_ERR(v3_client));
		return PTR_ERR(v3_client);
	}

	pr_info("minisforum_v3_accel: %s @ %s 0x%02x irq %d, %s matrix; st_lsm6dsx_i2c should bind now\n",
		V3_IMU_NAME, dev_name(&adap->dev), info.addr, info.irq,
		firmware_matrix ? "firmware SLA0" : "identity");
	request_module("i2c:" V3_IMU_NAME);
	return 0;
}

static void __exit v3_accel_exit(void)
{
	i2c_unregister_device(v3_client);
}

module_init(v3_accel_init);
module_exit(v3_accel_exit);

MODULE_DESCRIPTION("Minisforum V3: attach st_lsm6dsx to the SMOCF05 accelerometer/gyro");
MODULE_LICENSE("GPL");
