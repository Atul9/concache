#![allow(unused)]
// #[derive(Debug)]

extern crate rand;

use rand::{thread_rng, Rng};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

const OSC: Ordering = Ordering::SeqCst;

#[derive(Debug)]
struct Node {
    key: Option<usize>,
    val: Option<Mutex<usize>>,
    next: AtomicPtr<Node>,
}

impl Node {
    fn new(key: Option<usize>, val: Option<Mutex<usize>>) -> Node {
        Node {
            key: key,
            val: val,
            next: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

#[derive(Debug)]
struct LinkedList {
    head: AtomicPtr<Node>,
    tail: AtomicPtr<Node>,
}

impl LinkedList {
    fn new() -> Self {
        let head = Box::new(Node::new(None, None));
        let tail = Box::into_raw(Box::new(Node::new(None, None)));
        head.next.store(tail, OSC);

        LinkedList {
            head: AtomicPtr::new(Box::into_raw(head)),
            tail: AtomicPtr::new(tail),
        }
    }

    fn insert(&self, key: usize, val: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let mut new_node = Box::new(Node::new(Some(key), Some(Mutex::new(val))));
        let mut left_node: *mut Node = ptr::null_mut();
        let mut right_node: *mut Node = ptr::null_mut();

        loop {
            right_node = self.search(key, &mut left_node, remove_nodes);

            if ((right_node != self.tail.load(OSC)) && (unsafe { &*right_node }.key == Some(key))) {
                let rn = unsafe { &*right_node };
                let mut mx = rn.val.as_ref().unwrap().lock().unwrap();
                let old = *mx;
                *mx = val;
                return Some(old);
            }

            new_node.next.store(right_node, OSC);

            let new_node_ptr = Box::into_raw(new_node);
            if unsafe { &*left_node }
                .next
                .compare_and_swap(right_node, new_node_ptr, OSC)
                == right_node
            {
                return None;
            }
            new_node = unsafe { Box::from_raw(new_node_ptr) };
        }
    }

    fn print(&self) {
        println!("");
        println!("Printing List");
        let mut next_node = unsafe { &*self.head.load(OSC) };
        println!("{:?}", next_node);

        loop {
            next_node = unsafe { &*next_node.next.load(OSC) };
            println!("{:?}", next_node);
            if next_node.next.load(OSC) == self.tail.load(OSC) {
                break;
            }
        }
    }

    fn get(&self, search_key: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let mut left_node: *mut Node = ptr::null_mut();
        let right_node = self.search(search_key, &mut left_node, remove_nodes);
        if right_node == self.tail.load(OSC) || unsafe { &*right_node }.key != Some(search_key) {
            None
        } else {
            unsafe { &*right_node }
                .val
                .as_ref()
                .map(|v| *v.lock().unwrap())
        }
    }

    fn delete(&self, search_key: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let mut left_node: *mut Node = ptr::null_mut();
        let mut right_node: *mut Node = ptr::null_mut();
        let mut right_node_next: *mut Node = ptr::null_mut();

        loop {
            right_node = self.search(search_key, &mut left_node, remove_nodes);
            if (right_node == self.tail.load(OSC)) || unsafe { &*right_node }.key != Some(search_key) {
                return None; //failed delete
            }
            right_node_next = unsafe { &*right_node }.next.load(OSC);
            if !Self::is_marked_reference(right_node_next) {
                if unsafe { &*right_node }
                    .next
                    .compare_and_swap(right_node_next, Self::get_marked_reference(right_node_next), OSC)
                    == right_node_next
                {
                    break;
                }
            }
        }

        //get value to return
        let rn = unsafe { &*right_node };
        let mx = rn.val.as_ref().unwrap().lock().unwrap();

        if unsafe { &*left_node }
            .next
            .compare_and_swap(right_node, right_node_next, OSC)
            != right_node
        {
            right_node = self.search(unsafe { &*right_node }.key.unwrap(), &mut left_node, remove_nodes);
        }
    
        Some(*mx) //successful delete
    }

    fn is_marked_reference(ptr: *mut Node) -> bool {
        (ptr as usize & 0x1) == 1
    }
    fn get_marked_reference(ptr: *mut Node) -> *mut Node {
        (ptr as usize | 0x1) as *mut Node
    }
    fn get_unmarked_reference(ptr: *mut Node) -> *mut Node {
        (ptr as usize & !0x1) as *mut Node
    }

    fn search(&self, search_key: usize, left_node: &mut *mut Node, remove_nodes: &mut Vec<*mut Node>) -> *mut Node {
        let mut left_node_next: *mut Node = ptr::null_mut();
        let mut right_node: *mut Node = ptr::null_mut();

        //search
        'search_again: loop {
            let mut t = self.head.load(OSC);
            let mut t_next = unsafe { &*t }.next.load(OSC);

            /* 1: Find left_node and right_node */
            loop {
                if !Self::is_marked_reference(t_next) {
                    *left_node = t;
                    left_node_next = t_next;
                }
                t = Self::get_unmarked_reference(t_next);
                if t == self.tail.load(OSC) {
                    break;
                }
                t_next = unsafe { &*t }.next.load(OSC);
                if !Self::is_marked_reference(t_next) && unsafe { &*t }.key >= Some(search_key) {
                    break;
                }
            }
            right_node = t;

            /* 2: Check nodes are adjacent */
            if left_node_next == right_node {
                if right_node != self.tail.load(OSC)
                    && Self::is_marked_reference(unsafe { &*right_node }.next.load(OSC))
                {
                    continue 'search_again;
                } else {
                    return right_node;
                }
            }

            /* 3: Remove one or more marked nodes */
            if unsafe { &**left_node }
                .next
                .compare_and_swap(left_node_next, right_node, OSC)
                == left_node_next
            {
                //drop all of the Nodes that we crossed over, 
                //we know nothing inside can be modified so we can just drop all of them with 
                //loop until we are at the right_node pointer

                //add to remove_nodes, to be removed
                let mut curr_node = left_node_next; //left_node_next is to be deleted, the ones after it are
                // println!("start curr_node: {:?}", curr_node);

                loop {
                    //start with left_node_next, then go to on until the right_node, but do use that one
                    assert_eq!(Self::is_marked_reference(curr_node), false);
                    remove_nodes.push(curr_node);
                    curr_node = unsafe { &*curr_node }.next.load(OSC);
                    assert_eq!(Self::is_marked_reference(curr_node), true);
                    curr_node = Self::get_unmarked_reference(curr_node); //we need unmarked to deref and comp to right_node
                    // println!("curr_node: {:?}", curr_node);
                    if curr_node == right_node {
                        break;
                    }
                }

                if right_node != self.tail.load(OSC)
                    && Self::is_marked_reference(unsafe { &*right_node }.next.load(OSC))
                {
                    continue 'search_again;
                } else {
                    return right_node;
                }
            }
        }
    }
}

struct Table {
    nbuckets: usize,
    map: Vec<LinkedList>,
    nitems: AtomicUsize,
}

impl Table {
    fn new(num_of_buckets: usize) -> Self {
        let mut t = Table {
            nbuckets: num_of_buckets,
            map: Vec::with_capacity(num_of_buckets),
            nitems: AtomicUsize::new(0),
        };

        for _ in 0..num_of_buckets {
            t.map.push(LinkedList::new());
        }

        t
    }

    fn insert(&self, key: usize, value: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let check = self.nitems.load(OSC);

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash: usize = hasher.finish() as usize;
        let index = hash % self.nbuckets;

        let ret = self.map[index].insert(key, value, remove_nodes);

        if ret.is_none() {
            self.nitems.fetch_add(1, OSC);
        }

        ret
    }

    fn get(&self, key: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash: usize = hasher.finish() as usize;
        let index = hash % self.nbuckets;

        self.map[index].get(key, remove_nodes)
    }

    // fn resize(&mut self, newsize: usize) {
    // }

    fn delete(&self, key: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash: usize = hasher.finish() as usize;
        let index = hash % self.nbuckets;

        let ret = self.map[index].delete(key, remove_nodes);
        //if not None then subtract 1 from nitems

        if ret.is_some() {
            self.nitems.fetch_sub(1, OSC);
        }

        ret
    }
}

struct MapHandle {
    map: Arc<Hashmap>,
    epoch_counter: Arc<AtomicUsize>,
}

impl MapHandle {
    fn insert(&self, key: usize, value: usize) -> Option<usize> {
        let mut remove_nodes: Vec<*mut Node> = Vec::new();

        self.epoch_counter.fetch_add(1, OSC);
        let ret = self.map.insert(key, value, &mut remove_nodes);
        self.epoch_counter.fetch_add(1, OSC);
        if remove_nodes.len() != 0 {
            self.free_nodes(&remove_nodes);
        }
        

        ret
    }

    fn get(&self, key: usize) -> Option<usize> {
        let mut remove_nodes: Vec<*mut Node> = Vec::new();

        self.epoch_counter.fetch_add(1, OSC);
        let ret = self.map.get(key, &mut remove_nodes);
        self.epoch_counter.fetch_add(1, OSC);
        if remove_nodes.len() != 0 {
            self.free_nodes(&remove_nodes);
        }
        

        ret
    }

    fn delete(&self, key: usize) -> Option<usize> {
        let mut remove_nodes: Vec<*mut Node> = Vec::new();

        self.epoch_counter.fetch_add(1, OSC);
        let ret = self.map.delete(key, &mut remove_nodes);
        self.epoch_counter.fetch_add(1, OSC);
        if remove_nodes.len() != 0 {
            self.free_nodes(&remove_nodes);
        }

        ret
    }

    fn free_nodes(&self, remove_nodes: &Vec<*mut Node>) {
        /*
        we just finished up out adding to vector, so now we are ready to spin the epoch,
        and free those nodes up from the vector, TODO change fn parameter to accept a
        vector instead.
        */

        //epoch set up, load all of the values
        let mut started = Vec::new();
        let handles_map = self.map.handles.read().unwrap();
        for h in handles_map.iter() {
            started.push(h.load(OSC));
        }
        for (i,h) in handles_map.iter().enumerate() {
            let mut check = h.load(OSC);
            while (check <= started[i]) && (check%2 == 1) {
                // println!("epoch is spinning");
                check = h.load(OSC);
                //do nothing
            }
            //now finished is greater than or equal to started
        }

        //physical deletion, epoch has rolled over so we are safe to proceed with physical deletion
        // epoch rolled over, so we know we have exclusive access to the node
        
        for to_drop in remove_nodes {
            // println!("dropping: {:?}", to_drop);
            let n = unsafe { Box::from_raw(*to_drop) };
            // let n = unsafe { Box::from_raw(*to_drop) }; //use this to test double drop --> crash
        }
    }
}

impl Clone for MapHandle {
    fn clone(&self) -> Self {
        let ret = Self {
            map: Arc::clone(&self.map),
            epoch_counter: Arc::new(AtomicUsize::new(0)),
        };

        let mut hashmap = &self.map;
        let mut handles_vec = hashmap.handles.write().unwrap(); //handles vector
        handles_vec.push(Arc::clone(&ret.epoch_counter));

        ret
    }
}

struct Hashmap {
    table: RwLock<Table>,
    handles: RwLock<Vec<Arc<AtomicUsize>>>, //(started, finished)
}

impl Hashmap {
    fn new() -> MapHandle {
        let new_hashmap = Hashmap {
            table: RwLock::new(Table::new(8)),
            handles: RwLock::new(Vec::new()),
        };
        let mut ret = MapHandle {
            map: Arc::new(new_hashmap),
            epoch_counter: Arc::new(AtomicUsize::new(0)),
        };

        //push the first maphandle into the epoch system
        let mut hashmap = Arc::clone(&ret.map);
        let mut handles_vec = hashmap.handles.write().unwrap();
        handles_vec.push(Arc::clone(&ret.epoch_counter));
        ret
    }

    fn insert(&self, key: usize, value: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let inner_table = self.table.read().unwrap();
        inner_table.insert(key, value, remove_nodes)
    }

    fn get(&self, key: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let inner_table = self.table.read().unwrap(); //need read access
        inner_table.get(key, remove_nodes)
    }

    // fn resize(&self, newsize: usize) {
    // }

    fn delete(&self, key: usize, remove_nodes: &mut Vec<*mut Node>) -> Option<usize> {
        let inner_table = self.table.read().unwrap();
        inner_table.delete(key, remove_nodes)
    }
}

fn main() {
    println!("Started.");
    let mut handle = Hashmap::new(); //changed this,
        let mut threads = vec![];
        let nthreads = 5;
        // let handle = MapHandle::new(Arc::clone(&new_hashmap).table.read().unwrap());
        for _ in 0..nthreads {
            let new_handle = handle.clone();

            threads.push(thread::spawn(move || {
                let num_iterations = 1000000;
                for _ in 0..num_iterations {
                    let mut rng = thread_rng();
                    let val = rng.gen_range(0, 128);
                    let two = rng.gen_range(0, 3);

                    if two % 3 == 0 {
                        new_handle.insert(val, val);
                    } else if two % 3 == 1 {
                        let v = new_handle.get(val);
                        if (v.is_some()) {
                            assert_eq!(v.unwrap(), val);
                        }
                    } else {
                        new_handle.delete(val);
                    }
                }
                assert_eq!(new_handle.epoch_counter.load(OSC), num_iterations*2);
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
    println!("Finished.");
}

#[cfg(test)]
mod tests {
    use super::*;
    /*
    the data produced is a bit strange because of the way I take mod to test only even values 
    are inserted so the end number of values should be n/2 (computer style) and the capacity 
    of the map should be equal to the greatest power of 2 less than n/2.
    */
    #[test]
    fn hashmap_concurr() {
        let mut handle = Hashmap::new(); //changed this,
        let mut threads = vec![];
        let nthreads = 5;
        // let handle = MapHandle::new(Arc::clone(&new_hashmap).table.read().unwrap());
        for _ in 0..nthreads {
            let new_handle = handle.clone();

            threads.push(thread::spawn(move || {
                let num_iterations = 100000;
                for _ in 0..num_iterations {
                    let mut rng = thread_rng();
                    let val = rng.gen_range(0, 128);
                    let two = rng.gen_range(0, 3);

                    if two % 3 == 0 {
                        new_handle.insert(val, val);
                    } else if two % 3 == 1 {
                        let v = new_handle.get(val);
                        if (v.is_some()) {
                            assert_eq!(v.unwrap(), val);
                        }
                    } else {
                        new_handle.delete(val);
                    }
                }
                assert_eq!(new_handle.epoch_counter.load(OSC), num_iterations*2);
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
    }

    #[test]
    fn hashmap_handle_cloning() {
        let mut handle = Arc::new(Hashmap::new()); //init with 16 bucket
        println!("{:?}", handle.epoch_counter);
        handle.insert(1, 3);
        assert_eq!(handle.get(1).unwrap(), 3);

        //create a new handle
        let new_handle = Arc::clone(&handle);
        assert_eq!(new_handle.get(1).unwrap(), 3);
        new_handle.insert(2, 5);

        assert_eq!(handle.get(2).unwrap(), 5);
    }

    #[test]
    fn hashmap_delete() {
        let mut handle = Hashmap::new();
        handle.insert(1, 3);
        handle.insert(2, 5);
        handle.insert(3, 8);
        handle.insert(4, 3);
        handle.insert(5, 4);
        handle.insert(6, 5);
        handle.insert(7, 3);
        handle.insert(8, 3);
        handle.insert(9, 3);
        handle.insert(10, 3);
        handle.insert(11, 3);
        handle.insert(12, 3);
        handle.insert(13, 3);
        handle.insert(14, 3);
        handle.insert(15, 3);
        handle.insert(16, 3);
        assert_eq!(handle.get(1).unwrap(), 3);
        assert_eq!(handle.delete(1).unwrap(), 3);
        assert_eq!(handle.get(1), None);
        assert_eq!(handle.delete(2).unwrap(), 5);
        assert_eq!(handle.delete(16).unwrap(), 3);
        assert_eq!(handle.get(16), None);
    }

    #[test]
    fn linkedlist_basics() {
        let mut remove_nodes: Vec<*mut Node> = Vec::new();

        let mut new_linked_list = LinkedList::new();

        println!("{:?}", new_linked_list);
        new_linked_list.insert(3, 2, &mut remove_nodes);
        new_linked_list.insert(3, 4, &mut remove_nodes);
        new_linked_list.insert(5, 8, &mut remove_nodes);
        new_linked_list.insert(4, 6, &mut remove_nodes);
        new_linked_list.insert(1, 8, &mut remove_nodes);
        new_linked_list.insert(6, 6, &mut remove_nodes);
        new_linked_list.print();

        assert_eq!(new_linked_list.get(3, &mut remove_nodes).unwrap(), 4);
        assert_eq!(new_linked_list.get(5, &mut remove_nodes).unwrap(), 8);
        assert_eq!(new_linked_list.get(2, &mut remove_nodes), None);
    }

    #[test]
    fn hashmap_basics() {
        let mut new_hashmap = Hashmap::new(); //init with 2 buckets
                                              //input values
        new_hashmap.insert(1, 1);
        new_hashmap.insert(2, 5);
        new_hashmap.insert(12, 5);
        new_hashmap.insert(13, 7);
        new_hashmap.insert(0, 0);

        // assert_eq!(new_hashmap.table.read().unwrap().map.capacity(), 4); //should be 4 after you attempt the 5th insert

        new_hashmap.insert(20, 3);
        new_hashmap.insert(3, 2);
        new_hashmap.insert(4, 1);

        assert_eq!(new_hashmap.insert(20, 5).unwrap(), 3); //repeated
        assert_eq!(new_hashmap.insert(3, 8).unwrap(), 2); //repeated
        assert_eq!(new_hashmap.insert(5, 5), None); //repeated

        let cln = Arc::clone(&new_hashmap.map);
        assert_eq!(cln.table.read().unwrap().nitems.load(OSC), 9);

        new_hashmap.insert(3, 8); //repeated
        // assert_eq!(new_hashmap.table.read().unwrap().map.capacity(), 8); //should be 8 after you attempt the 9th insert

        assert_eq!(new_hashmap.get(20).unwrap(), 5);
        assert_eq!(new_hashmap.get(12).unwrap(), 5);
        assert_eq!(new_hashmap.get(1).unwrap(), 1);
        assert_eq!(new_hashmap.get(0).unwrap(), 0);
        assert!(new_hashmap.get(3).unwrap() != 2); // test that it changed

        // new_hashmap.resize(64);

        // assert_eq!(new_hashmap.table.read().unwrap().map.capacity(), 64); //make sure it is correct length

        // try the same assert_eqs
        assert_eq!(new_hashmap.get(20).unwrap(), 5);
        assert_eq!(new_hashmap.get(12).unwrap(), 5);
        assert_eq!(new_hashmap.get(1).unwrap(), 1);
        assert_eq!(new_hashmap.get(0).unwrap(), 0);
        assert!(new_hashmap.get(3).unwrap() != 2); // test that it changed
    }

    #[test]
    fn more_linked_list_tests() {
        let mut remove_nodes: Vec<*mut Node> = Vec::new();
        
        let mut new_linked_list = LinkedList::new();
        println!("Insert: {:?}", new_linked_list.insert(5, 3, &mut remove_nodes));
        println!("Insert: {:?}", new_linked_list.insert(5, 8, &mut remove_nodes));
        println!("Insert: {:?}", new_linked_list.insert(2, 3, &mut remove_nodes));


        println!("Get: {:?}", new_linked_list.get(5, &mut remove_nodes));

        // println!("{:?}", new_linked_list.head.load(OSC));
        new_linked_list.print();

        new_linked_list.delete(5, &mut remove_nodes);

        new_linked_list.print();
    }
}
