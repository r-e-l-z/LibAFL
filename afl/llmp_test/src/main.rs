#[macro_use]
extern crate alloc;

use core::convert::TryInto;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use std::thread;
use std::time;

use afl::events::llmp_translated::*;

const TAG_SIMPLE_U32_V1: u32 = 0x51300321;
const TAG_MATH_RESULT_V1: u32 = 0x77474331;

unsafe fn llmp_test_clientloop(client: *mut llmp_client, _data: *mut c_void) -> ! {
    let mut counter: u32 = 0;
    loop {
        counter += 1;

        let llmp_message = llmp_client_alloc_next(client, size_of::<u32>());
        core::ptr::copy(
            counter.to_be_bytes().as_ptr(),
            (*llmp_message).buf.as_mut_ptr(),
            size_of::<u32>(),
        );
        (*llmp_message).tag = TAG_SIMPLE_U32_V1;
        llmp_client_send(client, llmp_message).unwrap();

        thread::sleep(time::Duration::from_millis(100));
    }
}

unsafe fn u32_from_msg(message: *const llmp_message) -> u32 {
    u32::from_be_bytes(
        alloc::slice::from_raw_parts((*message).buf.as_ptr(), size_of::<u32>())
            .try_into()
            .unwrap(),
    )
}

unsafe fn test_adder_clientloop(client: *mut llmp_client, _data: *mut c_void) -> ! {
    let mut last_result: u32 = 0;
    let mut current_result: u32 = 0;
    loop {
        let mut msg_counter = 0;
        loop {
            let last_msg = llmp_client_recv(client);
            if last_msg == 0 as *mut llmp_message {
                break;
            }
            msg_counter += 1;
            match (*last_msg).tag {
                TAG_SIMPLE_U32_V1 => {
                    current_result = current_result.wrapping_add(u32_from_msg(last_msg));
                }
                _ => println!("Adder Client ignored unknown message {}", (*last_msg).tag),
            };
        }

        if current_result != last_result {
            println!(
                "Adder handled {} messages, reporting {} to broker",
                msg_counter, current_result
            );

            let llmp_message = llmp_client_alloc_next(client, size_of::<u32>());
            core::ptr::copy(
                current_result.to_be_bytes().as_ptr(),
                (*llmp_message).buf.as_mut_ptr(),
                size_of::<u32>(),
            );
            (*llmp_message).tag = TAG_MATH_RESULT_V1;
            llmp_client_send(client, llmp_message).unwrap();
            last_result = current_result;
        }

        thread::sleep(time::Duration::from_millis(100));
    }
}

unsafe fn broker_message_hook(
    _broker: *mut LlmpBroker,
    client_metadata: *mut llmp_broker_client_metadata,
    message: *mut llmp_message,
    _data: *mut c_void,
) -> LlmpMessageHookResult {
    match (*message).tag {
        TAG_SIMPLE_U32_V1 => {
            println!(
                "Client {:?} sent message: {:?}",
                (*client_metadata).pid,
                u32_from_msg(message)
            );
            LlmpMessageHookResult::ForwardToClients
        }
        TAG_MATH_RESULT_V1 => {
            println!(
                "Adder Client has this current result: {:?}",
                u32_from_msg(message)
            );
            LlmpMessageHookResult::Handled
        }
        _ => {
            println!("Unknwon message id received!");
            LlmpMessageHookResult::ForwardToClients
        }
    }
}

fn main() {
    /* The main node has a broker, and a few worker threads */
    let threads_total = num_cpus::get();

    let counter_thread_count = threads_total - 2;
    println!(
        "Running with 1 broker, 1 adder, and {} counter clients",
        counter_thread_count
    );

    unsafe {
        let mut broker = LlmpBroker::new().expect("Failed to create llmp broker");
        for i in 0..counter_thread_count {
            println!("Adding client {}", i);
            broker
                .register_childprocess_clientloop(llmp_test_clientloop, ptr::null_mut())
                .expect("could not add child clientloop");
        }

        broker
            .register_childprocess_clientloop(test_adder_clientloop, ptr::null_mut())
            .expect("Error registering childprocess");

        println!("Spawning broker");

        broker.add_message_hook(broker_message_hook, ptr::null_mut());

        broker.run();
    }
}