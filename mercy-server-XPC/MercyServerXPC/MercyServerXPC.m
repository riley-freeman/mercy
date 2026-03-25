#import <Foundation/Foundation.h>
#include <mach/mach.h>

#import "MercyServerXPC.h"

// ──────────────────────────────────────────────
// Allocation tracking
// ──────────────────────────────────────────────

typedef struct {
    void *ptr;
    size_t size;
} Allocation;

static NSMutableDictionary<NSNumber *, NSValue *> *allocations;
static NSInteger nextBlockId = 1;

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Decode a 16-byte alloc_id and return the blockId.
/// Layout: [implID: u16][blockID: u16][length: u32][familyID: u64]
static uint16_t decode_alloc_id(uint64_t allocIdLow) {
    return (uint16_t)(allocIdLow >> 16);
}

/// Build an XPC reply dictionary with the standard envelope fields.
static xpc_object_t create_reply_envelope(int64_t originalId, const char *messageType) {
    xpc_object_t reply = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_int64(reply, "id", originalId);
    xpc_dictionary_set_int64(reply, "reply_id", originalId);
    xpc_dictionary_set_string(reply, "message_type", messageType);
    return reply;
}

typedef struct {
    uint64_t high;  // family_id
    uint64_t low;   // size | block_id | implementation
} alloc_id_t;

/// Encode an alloc_id into out[16].
/// Layout: [implID: u16 = 0][blockID: u16][length: u32][familyID: u64]
static alloc_id_t encode_alloc_id(uint64_t family_id, uint16_t block_id, uint32_t length) {
    alloc_id_t id;
    id.high = family_id;
    id.low  = ((uint64_t)length << 32) |
        ((uint64_t)block_id << 16) |
        ((uint64_t)0);
    return id;
}

// ──────────────────────────────────────────────
// Message handlers
// ──────────────────────────────────────────────

static void handle_alloc(xpc_connection_t conn, int64_t msgId, xpc_object_t data) {
    int64_t familyId = xpc_dictionary_get_int64(data, "family_id");
    int64_t size = xpc_dictionary_get_int64(data, "size");
    if (size <= 0) return;

    // Allocate page-aligned memory suitable for xpc_shmem
    vm_address_t address = 0;
    kern_return_t kr = vm_allocate(mach_task_self(), &address, (vm_size_t)size, VM_FLAGS_ANYWHERE);
    if (kr != KERN_SUCCESS) return;

    // Store the allocation
    NSInteger blockId = nextBlockId ++;
    Allocation alloc = { .ptr = (void *)address, .size = (size_t)size };
    allocations[@(blockId)] = [NSValue valueWithBytes:&alloc objCType:@encode(Allocation)];

    // Build and send reply
    xpc_object_t reply = create_reply_envelope(msgId, "Alloc");
    xpc_object_t replyData = xpc_dictionary_create(NULL, NULL, 0);
    
    alloc_id_t alloc_id = encode_alloc_id((uint64_t)familyId, (uint16_t)blockId, (uint32_t)size);

    xpc_dictionary_set_uint64(replyData, "alloc_id_high", alloc_id.high);
    xpc_dictionary_set_uint64(replyData, "alloc_id_low", alloc_id.low);

    xpc_dictionary_set_value(reply, "message_data", replyData);
    xpc_connection_send_message(conn, reply);

    // xpc_release(replyData);
    // xpc_release(reply);
}

static void handle_free(int64_t msgId, xpc_object_t data) {
    alloc_id_t allocId = {
        xpc_dictionary_get_uint64(data, "alloc_id_high"),
        xpc_dictionary_get_uint64(data, "alloc_id_low"),
    };

    NSInteger blockId = (NSInteger)decode_alloc_id(allocId.low);
    NSValue *val = allocations[@(blockId)];
    if (!val) return;

    Allocation alloc;
    [val getValue:&alloc];

    vm_deallocate(mach_task_self(), (vm_address_t)alloc.ptr, alloc.size);
    [allocations removeObjectForKey:@(blockId)];
}

static void handle_map_id(xpc_connection_t conn, int64_t msgId, xpc_object_t data) {
    NSLog(@"[DEBUG] [MERCY SERVER] Sending over a xpc_shmem_object!\n");
    
    alloc_id_t allocId = {
        xpc_dictionary_get_uint64(data, "alloc_id_high"),
        xpc_dictionary_get_uint64(data, "alloc_id_low"),
    };
    NSInteger blockId = (NSInteger)decode_alloc_id(allocId.low);
    NSValue *val = allocations[@(blockId)];
    if (!val) return;

    Allocation alloc;
    [val getValue:&alloc];

    xpc_object_t shmem = xpc_shmem_create(alloc.ptr, alloc.size);
    if (!shmem) return;

    xpc_object_t reply = create_reply_envelope(msgId, "MapId");
    xpc_object_t replyData = xpc_dictionary_create(NULL, NULL, 0);

    xpc_dictionary_set_value(replyData, "xpc_shmem", shmem);
    xpc_dictionary_set_value(reply, "message_data", replyData);
    xpc_connection_send_message(conn, reply);

    NSLog(@"[DEBUG] [MERCY SERVER] Sent over a xpc_shmem_object!\n");
}

// ──────────────────────────────────────────────
// Event handler
// ──────────────────────────────────────────────

void xpc_event_handler(xpc_connection_t conn, xpc_object_t obj) {
    // Ignore non-dictionary objects (errors, etc.)
    if (xpc_get_type(obj) != XPC_TYPE_DICTIONARY) return;

    int64_t msgId = xpc_dictionary_get_int64(obj, "id");
    const char *messageType = xpc_dictionary_get_string(obj, "message_type");
    xpc_object_t data = xpc_dictionary_get_value(obj, "message_data");

    if (!messageType || !data) return;

    if (strcmp(messageType, "Alloc") == 0) {
        handle_alloc(conn, msgId, data);
    } else if (strcmp(messageType, "Free") == 0) {
        handle_free(msgId, data);
    } else if (strcmp(messageType, "MapId") == 0) {
        handle_map_id(conn, msgId, data);
    }
}

void xpc_connection_handler(xpc_connection_t conn) {
    xpc_connection_set_event_handler(conn, ^void(xpc_object_t obj) {
        xpc_event_handler(conn, obj);
    });
    xpc_connection_resume(conn);
}

int mercy_server_xpc_main(int argc, const char *argv[])
{
    allocations = [NSMutableDictionary new];
    xpc_main(xpc_connection_handler);
    return 0;
}

